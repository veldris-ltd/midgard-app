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
//!              [--max-connections N] [--idle-timeout SECONDS]
//!              [--rate-burst N] [--rate-per-sec N] [--requests-per-conn N]
//!              [--max-tokens N] [--max-prompt-bytes N] [--max-concurrent N]
//!              [--max-concurrent-per-identity N] [--infer-timeout SECONDS]
//!              [--reload-interval SECONDS] [--revocation-file FILE]
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
//!
//! The mesh listener serves at most `--max-connections` connections at once (default
//! 128), and closes one that sends nothing for `--idle-timeout` seconds (default 30).
//! Each certified peer may make `--rate-burst` requests in a burst (default 100),
//! refilled at `--rate-per-sec` a second (default 20; a burst and a rate of 0 turn the
//! limit off), and a connection is closed after `--requests-per-conn` requests (default
//! 10000; 0 is unlimited). A value that is not a number is a usage error.
//! [VLD-ANB-MIDGARD-0004]
//!
//! Every permitted call is admitted against an inference budget. `max_tokens` is
//! clamped to `--max-tokens` (default 1024); a prompt over `--max-prompt-bytes` (default
//! 262144) is refused `PROMPT_TOO_LARGE`; and a call over `--max-concurrent` calls in
//! flight (default 4, twice the sidecar's two slots) or `--max-concurrent-per-identity`
//! for its caller (default 2) is refused `BUSY`. Each refusal is audited and counted as
//! `inference_refused_total{reason}`, as is a request over the rate limit
//! (`reason="throttled"`). A completion waits at most `--infer-timeout` seconds (default
//! 60) on any read from the sidecar, after which it fails and its slot is free. Each of
//! these must be a whole number of at least 1. [VLD-ANB-MIDGARD-0016]
//!
//! Serving can be disengaged while the service runs, through files in the locked
//! `--state-dir`; there is no network control surface. Every `--reload-interval`
//! seconds (default 60, at least 1) the service looks for the stop file `STOP` there
//! and, for a model engine, re-reads the revocation list (`--revocation-file`, default
//! `revoked.list` in `--state-dir`; no file revokes nothing; one artefact id per line,
//! `#` comments) and `--model-trust`, and re-checks the manifest it verified at start-up
//! against them (its signature and gates; the weights are not hashed again). While the
//! stop file is present, inference is refused `SERVING_STOPPED`; while the model's id is
//! revoked, or its manifest no longer verifies against the trust key (or either file
//! cannot be read), it is refused `MODEL_REVOKED`. Either way `/readyz` reports
//! `model-verified` not ready, each refusal is audited and counted in
//! `inference_refused_total`, the log says `serving stopped` with the reason, and once
//! the completions already admitted have returned (waiting at most `--infer-timeout`)
//! it says `drained`. When every check passes again, serving resumes (`serving
//! resumed`). The reference engine honours the stop file only. [VLD-ANB-MIDGARD-0057]

use std::collections::BTreeSet;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gateway::{Gateway, RequestHandler};
use governance::Governance;
use model_server::{
    parse_revocation_list, parse_trust_key, probe, reference_registry, stage_verified_weights,
    withdraw_staged, Clock, FamilyAdapter, GemmaAdapter, GraniteAdapter, InferenceHandler,
    InferenceLimits, LicencePolicy, MistralAdapter, ModelBackend, ModelLoader, NemotronAdapter,
    NodeProfile, ReleaseFloor, Server, ServingState, SidecarBackend, StagedWeights, MODEL_REVOKED,
    SERVING_STOPPED,
};
use policy::{
    ClassificationLevel, DefaultPdp, Marking, PolicyDecisionPoint, Purpose, Resource, ResourceId,
};
use pqc_agility::backend::fips::provider as fips_provider;
use service_runtime::policy_file::parse_policy;
use service_runtime::{
    apply_mesh_limits, mesh_limits_from_args, serve_mesh_with, spawn_health_port, Args, DirLock,
    MeshLimits, ServiceIdentity,
};
use telemetry::{HealthRegistry, Logger, MetricsRegistry, ReadinessCheck, StdoutJsonSink, Toggle};

const USAGE: &str = "usage: mimir-server --identity-dir DIR --state-dir DIR --purposes P[,P...] \
                     [--service NAME] [--bind ADDR] [--health-bind ADDR] \
                     [--engine reference|llama-cpp|vllm --model-dir DIR --model-trust FILE \
                     --serve-dir DIR [--sidecar ADDR]] \
                     [--max-connections N] [--idle-timeout SECONDS] [--rate-burst N] \
                     [--rate-per-sec N] [--requests-per-conn N] \
                     [--max-tokens N] [--max-prompt-bytes N] [--max-concurrent N] \
                     [--max-concurrent-per-identity N] [--infer-timeout SECONDS] \
                     [--reload-interval SECONDS] [--revocation-file FILE]";

/// The id of the service's audit ledger.
const LEDGER_ID: &str = "mimir-inference";

/// The release floor of the models this service has accepted, in its state directory.
/// [MG-SEC-14]
const MODEL_FLOOR_FILE: &str = "model-release.floor";

/// How long a completion may wait on the sidecar for any one read, by default.
/// [VLD-ANB-MIDGARD-0016]
const DEFAULT_INFER_TIMEOUT_SECS: u64 = 60;

/// The operator's stop switch, in the state directory: while it exists nothing is
/// served. [VLD-ANB-MIDGARD-0057]
const STOP_FILE: &str = "STOP";

/// The revocation list, in the state directory unless `--revocation-file` says
/// otherwise. [VLD-ANB-MIDGARD-0057]
const REVOCATION_FILE: &str = "revoked.list";

/// How often the stop file, the revocation list and the trust key are re-read, by
/// default: the cadence the mesh CRL is reloaded at. [VLD-ANB-MIDGARD-0057]
const DEFAULT_RELOAD_INTERVAL_SECS: u64 = 60;

/// The inference budget, the sidecar timeout and the serving watch's reload interval
/// the command line asks for. [VLD-ANB-MIDGARD-0016] [VLD-ANB-MIDGARD-0057]
struct Admission {
    limits: InferenceLimits,
    infer_timeout: Duration,
    reload_interval: Duration,
}

/// [`InferenceLimits::default`] and a 60 s inference timeout, with what
/// `--max-tokens`, `--max-prompt-bytes`, `--max-concurrent`,
/// `--max-concurrent-per-identity` and `--infer-timeout` say instead. Each must be a
/// whole number of at least 1. The same holds for `--reload-interval` (default 60 s).
/// [VLD-ANB-MIDGARD-0016] [VLD-ANB-MIDGARD-0057]
fn admission_from_args(args: &Args) -> Result<Admission, String> {
    let positive = |key: &str| -> Result<Option<u64>, String> {
        args.get(key)
            .map(|v| match v.parse::<u64>() {
                Ok(n) if n >= 1 => Ok(n),
                _ => Err(format!(
                    "--{key} must be a whole number of at least 1, not {v:?}"
                )),
            })
            .transpose()
    };
    let mut limits = InferenceLimits::default();
    if let Some(n) = positive("max-tokens")? {
        limits.max_tokens = u32::try_from(n).unwrap_or(u32::MAX);
    }
    let size = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
    if let Some(n) = positive("max-prompt-bytes")? {
        limits.max_prompt_bytes = size(n);
    }
    if let Some(n) = positive("max-concurrent")? {
        limits.max_concurrent = size(n);
    }
    if let Some(n) = positive("max-concurrent-per-identity")? {
        limits.max_concurrent_per_identity = size(n);
    }
    let secs = positive("infer-timeout")?.unwrap_or(DEFAULT_INFER_TIMEOUT_SECS);
    let reload = positive("reload-interval")?.unwrap_or(DEFAULT_RELOAD_INTERVAL_SECS);
    Ok(Admission {
        limits,
        infer_timeout: Duration::from_secs(secs),
        reload_interval: Duration::from_secs(reload),
    })
}

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

/// What the serving watch re-checks of the served model: the manifest file's bytes as
/// they were verified at start-up, against the trust key and revocation list as they are
/// now. [VLD-ANB-MIDGARD-0057]
struct ModelRecheck {
    id: String,
    manifest_bytes: Vec<u8>,
    trust_path: PathBuf,
    revocation_file: PathBuf,
    node: NodeProfile,
}

impl ModelRecheck {
    /// Why the model may no longer be served, if it may not. An unreadable or malformed
    /// trust key or revocation list fails closed.
    fn refusal(&self) -> Option<String> {
        let revoked = match std::fs::read_to_string(&self.revocation_file) {
            Ok(text) => match parse_revocation_list(&text) {
                Ok(revoked) => revoked,
                Err(e) => return Some(format!("{}: {e}", self.revocation_file.display())),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(e) => {
                return Some(format!(
                    "cannot read {}: {e}",
                    self.revocation_file.display()
                ))
            }
        };
        let trust = match read_trust_key(&self.trust_path) {
            Ok(trust) => trust,
            Err(e) => return Some(e),
        };
        ModelLoader::new(trust, LicencePolicy::default())
            .with_revoked(revoked)
            .reverify_manifest(&self.manifest_bytes, &self.node)
            .err()
            .map(|e| format!("model {}: {e}", self.id))
    }
}

/// The serving control plane: on every reload interval it looks for the operator's stop
/// file and re-checks the model, and stops or resumes serving to match: the handler's
/// [`ServingState`] and the `model-verified` readiness check together, so a deployment
/// with or without an orchestrator stops. [VLD-ANB-MIDGARD-0057]
struct ServingWatch {
    stop_file: PathBuf,
    model: Option<ModelRecheck>,
    state: Arc<ServingState>,
    model_ready: Arc<Toggle>,
    drain_timeout: Duration,
}

impl ServingWatch {
    /// The reason code and the finding if serving must stop now. The stop file wins.
    fn verdict(&self) -> Option<(&'static str, String)> {
        match self.stop_file.try_exists() {
            Ok(false) => {}
            Ok(true) => {
                let found = format!("the stop file {} is present", self.stop_file.display());
                return Some((SERVING_STOPPED, found));
            }
            Err(e) => {
                let found = format!("cannot check {}: {e}", self.stop_file.display());
                return Some((SERVING_STOPPED, found));
            }
        }
        let refusal = self.model.as_ref()?.refusal()?;
        Some((MODEL_REVOKED, refusal))
    }

    /// One pass: stop (and drain) or resume as the checks say.
    fn pass(&self, logger: &Logger) {
        match self.verdict() {
            Some((code, found)) => {
                if self.state.reason().as_deref() == Some(code) {
                    return;
                }
                let was_serving = self.state.is_serving();
                self.state.stop(code);
                self.model_ready.set(false);
                logger.warn("serving stopped", &[("reason", code), ("detail", &found)]);
                if was_serving {
                    self.drain(logger);
                }
            }
            None if !self.state.is_serving() => {
                self.state.resume();
                self.model_ready.set(true);
                logger.info("serving resumed", &[]);
            }
            None => {}
        }
    }

    /// Wait, at most the inference timeout, for the completions admitted before the stop
    /// to return.
    fn drain(&self, logger: &Logger) {
        let admitted = self.state.in_flight().to_string();
        let deadline = Instant::now() + self.drain_timeout;
        while self.state.in_flight() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        match self.state.in_flight() {
            0 => logger.info("drained", &[("completions", &admitted)]),
            left => logger.warn(
                "drain timed out",
                &[("completions", &admitted), ("in_flight", &left.to_string())],
            ),
        }
    }

    /// Check now, then on every `interval` on a thread of its own.
    fn spawn(self, service: &str, interval: Duration, logger: &Logger) -> Result<(), String> {
        self.pass(logger);
        let service = service.to_string();
        std::thread::Builder::new()
            .name("serving-watch".into())
            .spawn(move || {
                let logger = Logger::new(&service, &StdoutJsonSink);
                loop {
                    std::thread::sleep(interval);
                    self.pass(&logger);
                }
            })
            .map(drop)
            .map_err(|e| format!("cannot start the serving watch: {e}"))
    }
}

/// The model-signing trust key in `path`.
fn read_trust_key(path: &Path) -> Result<pqc_agility::VerifyingKey, String> {
    std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
        .and_then(|text| parse_trust_key(&text).map_err(|e| format!("{}: {e}", path.display())))
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
    service_runtime::harden_process("mimir-server");
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
            "max-connections",
            "idle-timeout",
            "rate-burst",
            "rate-per-sec",
            "requests-per-conn",
            "max-tokens",
            "max-prompt-bytes",
            "max-concurrent",
            "max-concurrent-per-identity",
            "infer-timeout",
            "reload-interval",
            "revocation-file",
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
    let limits = match mesh_limits_from_args(&args) {
        Ok(limits) => limits,
        Err(e) => {
            eprintln!("mimir-server: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let admission = match admission_from_args(&args) {
        Ok(admission) => admission,
        Err(e) => {
            eprintln!("mimir-server: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(&args, &limits, &admission) {
        eprintln!("mimir-server: {e}");
        std::process::exit(1);
    }
}

/// Verify the model directory, at the release floor kept in `state_dir`, stage the
/// verified weights into the serve directory for the sidecar, and build the sidecar
/// backend that serves them, with the staged weights (held open), the log fields that
/// name what is served and what the serving watch re-checks of it. [MG-SEC-14]
/// [VLD-ANB-MIDGARD-0057]
fn sidecar_backend(
    args: &Args,
    engine: EngineChoice,
    state_dir: &Path,
    infer_timeout: Duration,
) -> Result<(Arc<SidecarBackend>, StagedWeights, LogFields, ModelRecheck), String> {
    let dir = Path::new(args.require("model-dir")?);
    let serve = serve_dir(args, dir)?;
    // Before anything is verified: a sidecar (re)started from here on waits for this
    // model to be staged. [MG-SEC-14]
    withdraw_staged(&serve).map_err(|e| format!("cannot use {}: {e}", serve.display()))?;
    let trust_path = Path::new(args.require("model-trust")?);
    let trust = read_trust_key(trust_path)?;
    let mut floor =
        ReleaseFloor::open(state_dir.join(MODEL_FLOOR_FILE)).map_err(|e| e.to_string())?;
    let node = probe();
    let mut model = ModelLoader::new(trust, LicencePolicy::default())
        .verify_dir_at_floor(dir, &node, &mut floor)?;
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
    .with_adapter(adapter)
    // A stuck completion fails at the inference timeout and frees its admission slot.
    // [VLD-ANB-MIDGARD-0016]
    .with_timeouts(Duration::from_millis(500), infer_timeout);

    let fields = vec![
        ("model", manifest.id.clone()),
        ("model_version", manifest.version.clone()),
        ("model_sha256", hex(&manifest.sha256)),
        ("model_staged", staged.path().display().to_string()),
        ("sidecar", addr.to_string()),
    ];
    let recheck = ModelRecheck {
        id: manifest.id.clone(),
        manifest_bytes: model.manifest_bytes,
        trust_path: trust_path.to_path_buf(),
        revocation_file: args
            .get("revocation-file")
            .map_or_else(|| state_dir.join(REVOCATION_FILE), PathBuf::from),
        node,
    };
    Ok((Arc::new(backend), staged, fields, recheck))
}

fn run(args: &Args, limits: &MeshLimits, admission: &Admission) -> Result<(), String> {
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
        for option in [
            "model-dir",
            "model-trust",
            "serve-dir",
            "sidecar",
            "revocation-file",
        ] {
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
    let health_bind = args.get("health-bind").unwrap_or("0.0.0.0:9090");
    if health_bind.starts_with("0.0.0.0") || health_bind.starts_with("[::]") {
        eprintln!(
            "mimir-server: WARNING health port binds every interface ({health_bind}); restrict \
             it with NetworkPolicy or a pod address. [VLD-ANB-MIDGARD-0027]"
        );
    }
    let health_addr = spawn_health_port(health_bind, Arc::new(health), metrics.clone())
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

    let (registry, model_fields, recheck): (Vec<Arc<dyn ModelBackend>>, LogFields, _) = match engine
    {
        EngineChoice::Reference => (reference_registry(), Vec::new(), None),
        _ => {
            let (sidecar, weights, fields, recheck) =
                sidecar_backend(args, engine, state_dir, admission.infer_timeout)?;
            let _ = sidecar_cell.set(sidecar.clone());
            let _ = weights_cell.set(weights);
            model_ready.set(true);
            (
                vec![sidecar as Arc<dyn ModelBackend>],
                fields,
                Some(recheck),
            )
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
    // Admission refusals and throttled requests are counted by reason.
    // [VLD-ANB-MIDGARD-0016]
    let refused = metrics.clone();
    let throttled = metrics.clone();
    let serving = Arc::new(ServingState::new());
    let handler: Arc<dyn RequestHandler> = Arc::new(
        InferenceHandler::new(
            Arc::new(server),
            governance,
            resource("mimir-inference"),
            Arc::new(WallClock),
        )
        .with_limits(admission.limits)
        .with_serving_state(serving.clone())
        .on_refusal(Arc::new(move |reason| {
            let reason = reason.to_ascii_lowercase();
            refused.inc("inference_refused_total", &[("reason", &reason)]);
        })),
    );
    let channel_pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(pdp()?);
    let gateway = apply_mesh_limits(
        Gateway::new(
            Arc::new(fips_provider()),
            identity.cert.clone(),
            identity.signing_key.clone(),
            identity.anchor.clone(),
            channel_pdp,
            resource("mimir-channel"),
            handler,
        )
        .on_throttle(Arc::new(move |_| {
            throttled.inc("inference_refused_total", &[("reason", "throttled")]);
        })),
        limits,
    );

    // The operator's stop file and the model's approval, checked before the first call
    // is accepted and on every reload interval after. [VLD-ANB-MIDGARD-0057]
    ServingWatch {
        stop_file: state_dir.join(STOP_FILE),
        model: recheck,
        state: serving,
        model_ready: model_ready.clone(),
        drain_timeout: admission.infer_timeout,
    }
    .spawn(service, admission.reload_interval, &logger)?;

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
    let limits_text = limits.to_string();
    let inference_text = format!(
        "{}, infer-timeout {}s",
        admission.limits,
        admission.infer_timeout.as_secs()
    );
    let reload_text = format!("{}s", admission.reload_interval.as_secs());
    let mut fields: Vec<(&str, &str)> = vec![
        ("mesh", &mesh),
        ("health", &health_text),
        ("roles", &roles),
        ("recovered_audit_entries", &recovered),
        ("engine", &engine_id),
        ("limits", &limits_text),
        ("inference_limits", &inference_text),
        ("reload_interval", &reload_text),
    ];
    fields.extend(model_fields.iter().map(|(k, v)| (*k, v.as_str())));
    logger.info("serving", &fields);
    serve_mesh_with(service, listener, gateway, metrics, limits.listener)
}
