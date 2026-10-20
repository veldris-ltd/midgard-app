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
//!               [--sidecar ADDR]]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME
//! defaults to `mimir-server`). `--state-dir` holds `policy.json` — the rules for
//! `OpenChannel` and `Infer` — and the service's ledger, `mimir.ledger`, and is locked
//! while the service runs. `--purposes` are the purposes inference may be called for.
//! PQ-mTLS is served on `--bind` (default `0.0.0.0:7720`); `/healthz`, `/readyz` and
//! `/metrics` on `--health-bind` (default `0.0.0.0:9090`).
//!
//! `--observe-bind ADDR` (off unless given) serves two read-only methods over PQ-mTLS
//! for the Mimir console's operators, each authorised and audited like a call:
//! `DescribeModels` (what is served, its signed manifest, the sidecar's health and
//! context, and throughput from recent calls) and `ReadSessions` (the recent calls,
//! each joined to its signed ledger entry, and the newest ledger entries). Policy must
//! let the caller's roles open the channel and perform those actions.
//!
//! `--engine` chooses what answers. `reference`, the default, is the deterministic
//! in-process engine. `llama-cpp` and `vllm` serve a real model through an
//! OpenAI-compatible sidecar on a loopback address (`--sidecar`, default
//! `127.0.0.1:8080`). Before anything is served, the model in `--model-dir` must pass
//! the load gate, or the service exits: a manifest signed by the key in `--model-trust`
//! (what `mg-model keygen` exported), a supported family this node can serve, and
//! weights whose size, format and SHA-256 match it. The service is ready only while the
//! sidecar reports healthy, so an orchestrator holds traffic while the sidecar loads.

use std::collections::BTreeSet;
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use gateway::{Gateway, RequestHandler, Router};
use governance::Governance;
use model_server::{
    parse_trust_key, probe, reference_registry, Clock, FamilyAdapter, GemmaAdapter, MistralAdapter,
    GraniteAdapter, InferenceHandler, LicencePolicy, ModelBackend, ModelLoader, NemotronAdapter,
    Server, SidecarBackend,
};
use policy::{
    ClassificationLevel, DefaultPdp, Marking, PolicyDecisionPoint, Purpose, Resource, ResourceId,
};
use pqc_agility::backend::fips::provider as fips_provider;
use service_runtime::json::{parse as parse_json, Json};
use service_runtime::policy_file::parse_policy;
use service_runtime::{serve_mesh, spawn_health_port, Args, DirLock, ServiceIdentity};
use telemetry::{HealthRegistry, Logger, MetricsRegistry, ReadinessCheck, StdoutJsonSink, Toggle};

const USAGE: &str = "usage: mimir-server --identity-dir DIR --state-dir DIR --purposes P[,P...] \
                     [--service NAME] [--bind ADDR] [--health-bind ADDR] [--observe-bind ADDR] \
                     [--engine reference|llama-cpp|vllm --model-dir DIR --model-trust FILE \
                     [--sidecar ADDR]]";

/// The id of the service's audit ledger.
const LEDGER_ID: &str = "mimir-inference";

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

/// The observer methods: [`DESCRIBE_MODELS`] and [`READ_SESSIONS`].
const DESCRIBE_MODELS: &str = "DescribeModels";
const READ_SESSIONS: &str = "ReadSessions";

fn observer_router<S: audit_writer::LedgerStore + Send + Sync + 'static>(
    inference: Arc<InferenceHandler<S>>,
    purpose: String,
    engine_id: String,
    described: Arc<OnceLock<Json>>,
    sidecar: Arc<OnceLock<Arc<SidecarBackend>>>,
) -> Router {
    let refuse = |m: &str| Json::obj(vec![("error", Json::s(m))]).encode().into_bytes();
    let (i1, p1) = (inference.clone(), purpose.clone());
    let models: Arc<dyn RequestHandler> =
        Arc::new(move |peer: &pq_mtls::WorkloadIdentity, _body: &[u8]| {
            if !i1.authorize_observer(peer, &p1, DESCRIBE_MODELS) {
                return refuse("policy does not permit this identity to describe the models");
            }
            describe_models(&i1, &engine_id, described.get(), sidecar.get())
                .encode()
                .into_bytes()
        });
    let i2 = inference;
    let sessions: Arc<dyn RequestHandler> =
        Arc::new(move |peer: &pq_mtls::WorkloadIdentity, body: &[u8]| {
            if !i2.authorize_observer(peer, &purpose, READ_SESSIONS) {
                return refuse("policy does not permit this identity to read the sessions");
            }
            let limit = std::str::from_utf8(body)
                .ok()
                .and_then(|t| parse_json(t).ok())
                .and_then(|j| j.get("limit").and_then(|v| v.as_i64()))
                .unwrap_or(100)
                .clamp(1, 500) as usize;
            read_sessions(&i2, limit).encode().into_bytes()
        });
    Router::new()
        .route(DESCRIBE_MODELS, models)
        .route(READ_SESSIONS, sessions)
}

fn describe_models<S: audit_writer::LedgerStore>(
    inference: &InferenceHandler<S>,
    engine_id: &str,
    described: Option<&Json>,
    sidecar: Option<&Arc<SidecarBackend>>,
) -> Json {
    let calls = inference.recent_calls();
    let served: Vec<_> = calls.iter().filter(|c| c.outcome == "served").collect();
    let tokens: u64 = served.iter().map(|c| c.tokens as u64).sum();
    let gen_ms: u64 = served
        .iter()
        .map(|c| c.total_ms.saturating_sub(c.first_token_ms as u64))
        .sum();
    let avg = |f: &dyn Fn(&&model_server::ServeStat) -> u64| {
        if served.is_empty() {
            Json::Null
        } else {
            Json::Int((served.iter().map(f).sum::<u64>() / served.len() as u64) as i64)
        }
    };
    let (ctx, slots) = sidecar
        .and_then(|s| sidecar_props(s.addr()))
        .unwrap_or((None, None));
    let node = probe();
    Json::obj(vec![
        ("engine", Json::s(engine_id)),
        (
            "model",
            described.cloned().unwrap_or(Json::obj(vec![
                ("id", Json::s(engine_id)),
                ("family", Json::s("reference")),
            ])),
        ),
        (
            "sidecar_healthy",
            sidecar
                .map(|s| Json::Bool(s.healthy()))
                .unwrap_or(Json::Null),
        ),
        (
            "ctx",
            ctx.map(|v| Json::Int(v as i64)).unwrap_or(Json::Null),
        ),
        (
            "slots",
            slots.map(|v| Json::Int(v as i64)).unwrap_or(Json::Null),
        ),
        (
            "node",
            Json::obj(vec![
                ("cpu_cores", Json::Int(node.cpu_cores as i64)),
                ("gpus", Json::Int(node.gpus as i64)),
                ("vram_gb_per_gpu", Json::Int(node.vram_gb_per_gpu as i64)),
                ("ram_gb", Json::Int(node.ram_gb as i64)),
                ("in_container", Json::Bool(node.in_container)),
            ]),
        ),
        ("calls", Json::Int(calls.len() as i64)),
        ("served", Json::Int(served.len() as i64)),
        (
            "failed",
            Json::Int(calls.iter().filter(|c| c.outcome == "failed").count() as i64),
        ),
        (
            "denied",
            Json::Int(calls.iter().filter(|c| c.outcome == "denied").count() as i64),
        ),
        ("avg_first_token_ms", avg(&|c| c.first_token_ms as u64)),
        ("avg_total_ms", avg(&|c| c.total_ms)),
        (
            "tokens_per_s",
            if gen_ms > 0 {
                Json::Num((tokens as f64 * 1000.0 / gen_ms as f64 * 10.0).round() / 10.0)
            } else {
                Json::Null
            },
        ),
        ("ledger_entries", Json::Int(inference.audit_len() as i64)),
    ])
}

fn read_sessions<S: audit_writer::LedgerStore>(
    inference: &InferenceHandler<S>,
    limit: usize,
) -> Json {
    let ledger = inference.recent_audit(2000);
    let entry = |seq: u64| ledger.iter().find(|e| e.seq == seq);
    let view = |e: &model_server::AuditView| {
        Json::obj(vec![
            ("seq", Json::Int(e.seq as i64)),
            ("actor", Json::s(&e.actor)),
            ("action", Json::s(&e.action)),
            ("resource", Json::s(&e.resource)),
            ("ts", Json::s(&e.ts)),
            ("entry_hash", Json::s(&e.entry_hash)),
            ("sig_alg", Json::s(&e.sig_alg)),
        ])
    };
    let sessions = inference
        .recent_calls()
        .into_iter()
        .rev()
        .take(limit)
        .map(|c| {
            Json::obj(vec![
                ("audit_seq", Json::Int(c.audit_seq as i64)),
                ("at", Json::Int(c.at as i64)),
                ("caller", Json::s(&c.caller)),
                ("purpose", Json::s(&c.purpose)),
                ("workload", Json::s(&c.workload)),
                (
                    "context",
                    c.context.as_deref().map(Json::s).unwrap_or(Json::Null),
                ),
                ("outcome", Json::s(&c.outcome)),
                (
                    "model",
                    c.model.as_deref().map(Json::s).unwrap_or(Json::Null),
                ),
                ("prompt_tokens", Json::Int(c.prompt_tokens as i64)),
                ("tokens", Json::Int(c.tokens as i64)),
                ("first_token_ms", Json::Int(c.first_token_ms as i64)),
                ("total_ms", Json::Int(c.total_ms as i64)),
                (
                    "detail",
                    c.detail.as_deref().map(Json::s).unwrap_or(Json::Null),
                ),
                ("ledger", entry(c.audit_seq).map(view).unwrap_or(Json::Null)),
            ])
        })
        .collect();
    Json::obj(vec![
        ("sessions", Json::Arr(sessions)),
        (
            "ledger",
            Json::Arr(ledger.iter().rev().take(50).map(view).collect()),
        ),
        ("ledger_entries", Json::Int(inference.audit_len() as i64)),
    ])
}

/// The sidecar's context size and slots from llama.cpp's `/props`, best effort.
fn sidecar_props(addr: SocketAddr) -> Option<(Option<u64>, Option<u64>)> {
    use std::io::{Read, Write};
    let mut s =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500)).ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .ok()?;
    s.write_all(format!("GET /props HTTP/1.0\r\nHost: {addr}\r\n\r\n").as_bytes())
        .ok()?;
    let mut raw = Vec::new();
    s.take(256 * 1024).read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw);
    let body = parse_json(text.split_once("\r\n\r\n")?.1).ok()?;
    let ctx = body
        .get("default_generation_settings")
        .and_then(|g| g.get("n_ctx"))
        .and_then(|v| v.as_i64())
        .map(|v| v as u64);
    let slots = body
        .get("total_slots")
        .and_then(|v| v.as_i64())
        .map(|v| v as u64);
    Some((ctx, slots))
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
            "observe-bind",
            "engine",
            "model-dir",
            "model-trust",
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

/// Verify the model directory and build the sidecar backend that serves it, with the
/// log fields that name what is served.
fn sidecar_backend(
    args: &Args,
    engine: EngineChoice,
) -> Result<(Arc<SidecarBackend>, LogFields, Json), String> {
    let dir = Path::new(args.require("model-dir")?);
    let trust_path = args.require("model-trust")?;
    let trust = std::fs::read_to_string(trust_path)
        .map_err(|e| format!("cannot read {trust_path}: {e}"))
        .and_then(|text| parse_trust_key(&text).map_err(|e| format!("{trust_path}: {e}")))?;
    let model = ModelLoader::new(trust, LicencePolicy::default()).verify_dir(dir, &probe())?;
    let manifest = &model.artifact.manifest;

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
        ("model_sha256", hex(&manifest.sha256)),
        ("sidecar", addr.to_string()),
    ];
    let w = manifest.weights.as_ref();
    let described = Json::obj(vec![
        ("id", Json::s(&manifest.id)),
        ("family", Json::s(&manifest.family)),
        ("version", Json::s(&manifest.version)),
        (
            "quantisation",
            w.map(|w| Json::s(&w.quantisation)).unwrap_or(Json::Null),
        ),
        (
            "format",
            w.map(|w| Json::s(w.format.as_str())).unwrap_or(Json::Null),
        ),
        (
            "size_bytes",
            w.map(|w| Json::Int(w.size as i64)).unwrap_or(Json::Null),
        ),
        ("file", w.map(|w| Json::s(&w.name)).unwrap_or(Json::Null)),
        ("sha256", Json::s(&hex(&manifest.sha256))),
        ("licence", Json::s(&manifest.licence.name)),
        ("osi_approved", Json::Bool(manifest.licence.osi_approved)),
        (
            "redistributable",
            Json::Bool(manifest.licence.redistribution_ok),
        ),
        ("min_gpus", Json::Int(manifest.hw.min_gpus as i64)),
        ("min_vram_gb", Json::Int(manifest.hw.min_vram_gb as i64)),
        ("signature", Json::s("ML-DSA verified on load")),
        (
            "engine",
            Json::s(match engine {
                EngineChoice::LlamaCpp => "llama-cpp",
                _ => "vllm",
            }),
        ),
        ("sidecar", Json::s(&addr.to_string())),
    ]);
    Ok((Arc::new(backend), fields, described))
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
        for option in ["model-dir", "model-trust", "sidecar"] {
            if args.get(option).is_some() {
                return Err(format!("--{option} needs --engine llama-cpp or vllm"));
            }
        }
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
    let described_cell: Arc<OnceLock<Json>> = Arc::new(OnceLock::new());
    let mut health = HealthRegistry::new();
    health.register(Box::new(ledger_ready.clone()));
    health.register(Box::new(mesh_ready.clone()));
    health.register(Box::new(model_ready.clone()));
    if engine != EngineChoice::Reference {
        health.register(Box::new(SidecarReady(sidecar_cell.clone())));
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
            let (sidecar, fields, described) = sidecar_backend(args, engine)?;
            let _ = sidecar_cell.set(sidecar.clone());
            let _ = described_cell.set(described);
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
    let inference = Arc::new(InferenceHandler::new(
        Arc::new(server),
        governance,
        resource("mimir-inference"),
        Arc::new(WallClock),
    ));
    let handler: Arc<dyn RequestHandler> = inference.clone();
    let observe_purpose = purposes
        .iter()
        .next()
        .map(|p| p.0.clone())
        .unwrap_or_default();
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
    // The observer port, when asked for: the console's read-only view of the plane.
    // [FR-MI-8]
    let mut observe_text = String::from("off");
    if let Some(bind) = args.get("observe-bind") {
        let observe_listener =
            TcpListener::bind(bind).map_err(|e| format!("cannot bind the observer port: {e}"))?;
        observe_text = observe_listener
            .local_addr()
            .map_err(|e| format!("the observer port has no address: {e}"))?
            .to_string();
        let router = observer_router(
            inference.clone(),
            observe_purpose,
            engine_id.clone(),
            described_cell.clone(),
            sidecar_cell.clone(),
        );
        let observe_gateway = Gateway::new(
            Arc::new(fips_provider()),
            identity.cert.clone(),
            identity.signing_key.clone(),
            identity.anchor.clone(),
            Arc::new(pdp()?),
            resource("mimir-observe"),
            Arc::new(router),
        );
        let observe_service = format!("{service}-observe");
        let observe_metrics = Arc::new(MetricsRegistry::new());
        std::thread::spawn(move || {
            serve_mesh(
                &observe_service,
                observe_listener,
                observe_gateway,
                observe_metrics,
            )
        });
    }
    fields.push(("observe", &observe_text));
    logger.info("serving", &fields);
    serve_mesh(service, listener, gateway, metrics)
}
