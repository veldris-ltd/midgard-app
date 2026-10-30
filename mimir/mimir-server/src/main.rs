//! `mimir-server` — the Mimir inference plane as a running service. [FR-MI-8] [FR-MI-1]
//! [AC-MI-7] [NFR-SEC-1] [RC-33] [RC-09]
//!
//! Inference is reachable only over PQ-mTLS: a peer must present a certificate from
//! the enclosure's issuer, and policy must let its roles open the channel and then
//! `Infer`. Every call — permitted or denied — is one PQ-signed entry in the service's
//! own sealed ledger, which is re-verified on every boot before anything is served.
//!
//! ```text
//! mimir-server --identity-dir DIR --state-dir DIR --purposes P[,P...]
//!              [--service NAME] [--bind ADDR] [--health-bind ADDR]
//!              [--engine reference|llama-cpp|vllm --model-dir DIR --model-trust FILE
//!               --serve-dir DIR [--sidecar ADDR]]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME
//! defaults to `mimir-server`). `--state-dir` holds `policy.json` — the rules for
//! `OpenChannel` and `Infer` — and the service's ledger, `mimir.ledger`, and is locked
//! while the service runs. `--purposes` are the purposes inference may be called for.
//! PQ-mTLS is served on `--bind` (default `0.0.0.0:7720`); `/healthz`, `/readyz` and
//! `/metrics` on `--health-bind` (default `0.0.0.0:9090`).
//!
//! `--engine` chooses what answers. `reference`, the default, is the deterministic
//! in-process engine. `llama-cpp` and `vllm` serve a real model through an
//! OpenAI-compatible sidecar on a loopback address (`--sidecar`, default
//! `127.0.0.1:8080`). Before anything is served, the model in `--model-dir` must pass
//! the load gate, or the service exits: a manifest signed by the key in `--model-trust`
//! (what `mg-model keygen` exported) and checked over its raw bytes before it is parsed,
//! a supported family this node can serve, and weights whose size, format and SHA-256
//! match it. A release older than the last one this service accepted for that model, or
//! the same version with different signed content, is refused: the floor is kept in
//! `--state-dir` as `model-release.floor`.
//!
//! The sidecar loads its weights by path, so it is never given the model directory.
//! The verified weights are staged into `--serve-dir`, a directory only this service
//! writes and the sidecar mounts read-only: copied from the handle they were verified
//! through, hashed again as they are copied, made read-only and renamed into place under
//! their signed file name, after which the marker `staged.ready` is written. The sidecar
//! is started on `DIR/<weights file>` once that marker exists; a file swapped into the
//! model directory after the check is never what it loads. The marker is withdrawn
//! before the model is verified, so a sidecar (re)started meanwhile waits. The service
//! is ready only while the sidecar reports healthy and the staged file is still the one
//! that was staged, so an orchestrator holds traffic while the sidecar loads. [MG-SEC-14]

use std::collections::BTreeSet;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use gateway::{Gateway, RequestHandler};
use governance::Governance;
use model_server::{
    parse_trust_key, probe, reference_registry, stage_verified_weights, withdraw_staged, Clock,
    FamilyAdapter, GemmaAdapter, GraniteAdapter, InferenceHandler, LicencePolicy, MistralAdapter,
    ModelBackend, ModelLoader, NemotronAdapter, ReleaseFloor, Server, SidecarBackend,
    StagedWeights,
};
use policy::{
    ClassificationLevel, DefaultPdp, Marking, PolicyDecisionPoint, Purpose, Resource, ResourceId,
};
use pqc_agility::backend::fips::provider as fips_provider;
use service_runtime::policy_file::parse_policy;
use service_runtime::{serve_mesh, spawn_health_port, Args, DirLock, ServiceIdentity};
use telemetry::{HealthRegistry, Logger, MetricsRegistry, ReadinessCheck, StdoutJsonSink, Toggle};

const USAGE: &str = "usage: mimir-server --identity-dir DIR --state-dir DIR --purposes P[,P...] \
                     [--service NAME] [--bind ADDR] [--health-bind ADDR] \
                     [--engine reference|llama-cpp|vllm --model-dir DIR --model-trust FILE \
                     --serve-dir DIR [--sidecar ADDR]]";

/// The id of the service's audit ledger.
const LEDGER_ID: &str = "mimir-inference";

/// The release floor of the models this service has accepted, in its state directory.
/// [MG-SEC-14]
const MODEL_FLOOR_FILE: &str = "model-release.floor";

/// Extra fields for the `serving` log line.
type LogFields = Vec<(&'static str, String)>;

/// Audit timestamps from the system clock, in seconds.
struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// What answers inference.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EngineChoice {
    Reference,
    LlamaCpp,
    Vllm,
}

/// Ready while the model's sidecar reports healthy, once there is one.
struct SidecarReady(Arc<OnceLock<Arc<SidecarBackend>>>);

impl ReadinessCheck for SidecarReady {
    fn name(&self) -> &str {
        "model-sidecar"
    }
    fn check(&self) -> bool {
        self.0.get().is_some_and(|sidecar| sidecar.healthy())
    }
}

/// Ready while the staged weights file is still the one that was staged, read-only, in
/// place: it is what the sidecar loads. [MG-SEC-14]
struct WeightsUnchanged(Arc<OnceLock<StagedWeights>>);

impl ReadinessCheck for WeightsUnchanged {
    fn name(&self) -> &str {
        "model-weights-unchanged"
    }
    fn check(&self) -> bool {
        self.0.get().is_some_and(StagedWeights::still_in_place)
    }
}

/// The serve directory: an existing directory that is neither the model directory nor
/// inside it, nor holds it. [MG-SEC-14]
fn serve_dir(args: &Args, model_dir: &Path) -> Result<PathBuf, String> {
    let serve = Path::new(args.require("serve-dir")?);
    let canonical = |path: &Path| {
        path.canonicalize()
            .map_err(|e| format!("cannot use {}: {e}", path.display()))
    };
    let (serve_abs, model_abs) = (canonical(serve)?, canonical(model_dir)?);
    if !serve_abs.is_dir() {
        return Err(format!(
            "--serve-dir {} is not a directory",
            serve.display()
        ));
    }
    if serve_abs.starts_with(&model_abs) || model_abs.starts_with(&serve_abs) {
        return Err(
            "--serve-dir must be apart from --model-dir: the sidecar must never see the \
             unverified model directory"
                .into(),
        );
    }
    Ok(serve.to_path_buf())
}

/// The chat template for a model family.
fn adapter_for(family: &str) -> Option<Box<dyn FamilyAdapter>> {
    Some(match family {
        "Mistral" => Box::new(MistralAdapter),
        "Nemotron" => Box::new(NemotronAdapter),
        "Gemma" => Box::new(GemmaAdapter),
        "Granite" => Box::new(GraniteAdapter),
        _ => return None,
    })
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(
        &argv,
        &[
            "identity-dir",
            "state-dir",
            "purposes",
            "service",
            "bind",
            "health-bind",
            "engine",
            "model-dir",
            "model-trust",
            "serve-dir",
            "sidecar",
        ],
        &[],
    ) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("mimir-server: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(&args) {
        eprintln!("mimir-server: {e}");
        std::process::exit(1);
    }
}

/// Verify the model directory, at the release floor kept in `state_dir`, stage the
/// verified weights into the serve directory for the sidecar, and build the sidecar
/// backend that serves them, with the staged weights (held open) and the log fields that
/// name what is served. [MG-SEC-14]
fn sidecar_backend(
    args: &Args,
    engine: EngineChoice,
    state_dir: &Path,
) -> Result<(Arc<SidecarBackend>, StagedWeights, LogFields), String> {
    let dir = Path::new(args.require("model-dir")?);
    let serve = serve_dir(args, dir)?;
    // Before anything is verified: a sidecar (re)started from here on waits for this
    // model to be staged. [MG-SEC-14]
    withdraw_staged(&serve).map_err(|e| format!("cannot use {}: {e}", serve.display()))?;
    let trust_path = args.require("model-trust")?;
    let trust = std::fs::read_to_string(trust_path)
        .map_err(|e| format!("cannot read {trust_path}: {e}"))
        .and_then(|text| parse_trust_key(&text).map_err(|e| format!("{trust_path}: {e}")))?;
    let mut floor =
        ReleaseFloor::open(state_dir.join(MODEL_FLOOR_FILE)).map_err(|e| e.to_string())?;
    let mut model = ModelLoader::new(trust, LicencePolicy::default()).verify_dir_at_floor(
        dir,
        &probe(),
        &mut floor,
    )?;
    let staged = stage_verified_weights(&mut model.weights, &model.manifest, &serve)?;
    let manifest = &model.manifest;

    let addr: SocketAddr = args
        .get("sidecar")
        .unwrap_or("127.0.0.1:8080")
        .parse()
        .map_err(|_| "--sidecar must be an address such as 127.0.0.1:8080".to_string())?;
    let adapter = adapter_for(&manifest.family)
        .ok_or_else(|| format!("no chat template for the {} family", manifest.family))?;
    let backend = match engine {
        EngineChoice::LlamaCpp => SidecarBackend::llama_cpp(
            format!("llama-cpp/{}", manifest.id),
            addr,
            manifest.id.clone(),
            &manifest.family,
        ),
        _ => SidecarBackend::vllm(
            format!("vllm/{}", manifest.id),
            addr,
            manifest.id.clone(),
            vec![manifest.family.clone()],
            manifest.hw,
            20,
        ),
    }
    .map_err(|e| e.to_string())?
    .with_adapter(adapter);

    let fields = vec![
        ("model", manifest.id.clone()),
        ("model_version", manifest.version.clone()),
        ("model_sha256", hex(&manifest.sha256)),
        ("model_staged", staged.path().display().to_string()),
        ("sidecar", addr.to_string()),
    ];
    Ok((Arc::new(backend), staged, fields))
}

fn run(args: &Args) -> Result<(), String> {
    let service = args.get("service").unwrap_or("mimir-server");
    let identity_dir = args.require("identity-dir")?;
    let state_dir = Path::new(args.require("state-dir")?);
    let purposes: BTreeSet<Purpose> = args
        .require("purposes")?
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| Purpose(p.to_string()))
        .collect();
    if purposes.is_empty() {
        return Err("--purposes needs at least one purpose".into());
    }
    let engine = match args.get("engine").unwrap_or("reference") {
        "reference" => EngineChoice::Reference,
        "llama-cpp" => EngineChoice::LlamaCpp,
        "vllm" => EngineChoice::Vllm,
        other => {
            return Err(format!(
                "--engine must be reference, llama-cpp or vllm, not {other:?}"
            ))
        }
    };
    if engine == EngineChoice::Reference {
        for option in ["model-dir", "model-trust", "serve-dir", "sidecar"] {
            if args.get(option).is_some() {
                return Err(format!("--{option} needs --engine llama-cpp or vllm"));
            }
        }
    } else {
        // The sidecar is only ever given the staged copy. [MG-SEC-14]
        args.require("serve-dir")?;
    }
    let logger = Logger::new(service, &StdoutJsonSink);

    // Probeable from the start, ready only once the ledger verifies, the model is
    // verified, the port is bound and any sidecar answers.
    let ledger_ready = Arc::new(Toggle::new("audit-ledger", false));
    let mesh_ready = Arc::new(Toggle::new("mesh-listener", false));
    let model_ready = Arc::new(Toggle::new(
        "model-verified",
        engine == EngineChoice::Reference,
    ));
    let sidecar_cell = Arc::new(OnceLock::new());
    let weights_cell = Arc::new(OnceLock::new());
    let mut health = HealthRegistry::new();
    health.register(Box::new(ledger_ready.clone()));
    health.register(Box::new(mesh_ready.clone()));
    health.register(Box::new(model_ready.clone()));
    if engine != EngineChoice::Reference {
        health.register(Box::new(SidecarReady(sidecar_cell.clone())));
        health.register(Box::new(WeightsUnchanged(weights_cell.clone())));
    }
    let metrics = Arc::new(MetricsRegistry::new());
    let health_addr = spawn_health_port(
        args.get("health-bind").unwrap_or("0.0.0.0:9090"),
        Arc::new(health),
        metrics.clone(),
    )
    .map_err(|e| format!("cannot bind the health port: {e}"))?;

    let _lock = DirLock::acquire(state_dir, "mimir.lock", service)?;
    let identity = ServiceIdentity::load(identity_dir, service)?;

    let policy_path = state_dir.join("policy.json");
    let policy_text = std::fs::read_to_string(&policy_path)
        .map_err(|e| format!("cannot read {}: {e}", policy_path.display()))?;
    let pdp = || -> Result<DefaultPdp, String> {
        parse_policy(&policy_text)
            .map(DefaultPdp::new)
            .map_err(|e| format!("{}: {e}", policy_path.display()))
    };

    let writer = identity.audit_writer(state_dir.join("mimir.ledger"), LEDGER_ID)?;
    let recovered = writer.entries().len();
    let governance = Governance::with_audit(pdp()?, writer);
    ledger_ready.set(true);

    let (registry, model_fields): (Vec<Arc<dyn ModelBackend>>, LogFields) = match engine {
        EngineChoice::Reference => (reference_registry(), Vec::new()),
        _ => {
            let (sidecar, weights, fields) = sidecar_backend(args, engine, state_dir)?;
            let _ = sidecar_cell.set(sidecar.clone());
            let _ = weights_cell.set(weights);
            model_ready.set(true);
            (vec![sidecar as Arc<dyn ModelBackend>], fields)
        }
    };
    let server = Server::bootstrap(probe(), &registry, None)
        .map_err(|e| format!("no model can be served on this node: {e}"))?;
    let engine_id = server.selection().backend_id.clone();
    let resource = |id: &str| Resource {
        id: ResourceId(id.into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: purposes.clone(),
    };
    let handler: Arc<dyn RequestHandler> = Arc::new(InferenceHandler::new(
        Arc::new(server),
        governance,
        resource("mimir-inference"),
        Arc::new(WallClock),
    ));
    let channel_pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(pdp()?);
    let gateway = Gateway::new(
        Arc::new(fips_provider()),
        identity.cert.clone(),
        identity.signing_key.clone(),
        identity.anchor.clone(),
        channel_pdp,
        resource("mimir-channel"),
        handler,
    );

    let listener = TcpListener::bind(args.get("bind").unwrap_or("0.0.0.0:7720"))
        .map_err(|e| format!("cannot bind the mesh port: {e}"))?;
    let mesh_addr = listener
        .local_addr()
        .map_err(|e| format!("the mesh port has no address: {e}"))?;
    mesh_ready.set(true);
    let (mesh, health_text, roles, recovered) = (
        mesh_addr.to_string(),
        health_addr.to_string(),
        identity.roles().join(","),
        recovered.to_string(),
    );
    let mut fields: Vec<(&str, &str)> = vec![
        ("mesh", &mesh),
        ("health", &health_text),
        ("roles", &roles),
        ("recovered_audit_entries", &recovered),
        ("engine", &engine_id),
    ];
    fields.extend(model_fields.iter().map(|(k, v)| (*k, v.as_str())));
    logger.info("serving", &fields);
    serve_mesh(service, listener, gateway, metrics)
}
