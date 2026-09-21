//! `heimdall-feed` — the Heimdall feed gateway as a running service. [FR-H-1] [FR-H-3]
//! [NFR-PQ-3] [RC-13] [RC-33] [NFR-SEC-1]
//!
//! ```text
//! heimdall-feed --identity-dir DIR --state-dir DIR
//!               [--service NAME] [--purpose P] [--bind ADDR] [--health-bind ADDR]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME defaults
//! to `heimdall-feed`). `--state-dir` holds `policy.json` — the rules for `OpenChannel`,
//! `Ingest` and `ReadPicture` — and the service's sealed `picture.sealed` and
//! `feed.ledger`; it is locked while the service runs. Requests are audited under
//! `--purpose` (default `situational-awareness`), the one purpose the channel opens
//! for. PQ-mTLS is served on `--bind` (default `0.0.0.0:7730`); `/healthz`, `/readyz`
//! and `/metrics` on `--health-bind` (default `0.0.0.0:9090`).

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, RwLock};

use gateway::Gateway;
use governance::Governance;
use heimdall_feed_server::{FeedService, PictureState};
use heimdall_store::DurablePicture;
use policy::{
    ClassificationLevel, DefaultPdp, Marking, PolicyDecisionPoint, Purpose, Resource, ResourceId,
};
use pqc_agility::backend::fips::provider as fips_provider;
use service_runtime::policy_file::parse_policy;
use service_runtime::{
    serve_mesh, spawn_health_port, unix_seconds, Args, DirLock, ServiceIdentity,
};
use telemetry::{HealthRegistry, Logger, MetricsRegistry, StdoutJsonSink, Toggle};

const USAGE: &str = "usage: heimdall-feed --identity-dir DIR --state-dir DIR [--service NAME] \
                     [--purpose P] [--bind ADDR] [--health-bind ADDR]";

/// The id of the service's audit ledger.
const LEDGER_ID: &str = "heimdall-feed-audit";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(
        &argv,
        &[
            "identity-dir",
            "state-dir",
            "service",
            "purpose",
            "bind",
            "health-bind",
        ],
        &[],
    ) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("heimdall-feed: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(&args) {
        eprintln!("heimdall-feed: {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    let service = args.get("service").unwrap_or("heimdall-feed");
    let identity_dir = args.require("identity-dir")?;
    let state_dir = Path::new(args.require("state-dir")?);
    let purpose = args.get("purpose").unwrap_or("situational-awareness");
    let logger = Logger::new(service, &StdoutJsonSink);

    let state_ready = Arc::new(Toggle::new("picture", false));
    let mesh_ready = Arc::new(Toggle::new("mesh-listener", false));
    let mut health = HealthRegistry::new();
    health.register(Box::new(state_ready.clone()));
    health.register(Box::new(mesh_ready.clone()));
    let metrics = Arc::new(MetricsRegistry::new());
    let health_addr = spawn_health_port(
        args.get("health-bind").unwrap_or("0.0.0.0:9090"),
        Arc::new(health),
        metrics.clone(),
    )
    .map_err(|e| format!("cannot bind the health port: {e}"))?;

    let _lock = DirLock::acquire(state_dir, "feed.lock", service)?;
    let identity = ServiceIdentity::load(identity_dir, service)?;

    let policy_path = state_dir.join("policy.json");
    let policy_text = std::fs::read_to_string(&policy_path)
        .map_err(|e| format!("cannot read {}: {e}", policy_path.display()))?;
    let pdp = || -> Result<DefaultPdp, String> {
        parse_policy(&policy_text)
            .map(DefaultPdp::new)
            .map_err(|e| format!("{}: {e}", policy_path.display()))
    };

    let writer = identity.audit_writer(state_dir.join("feed.ledger"), LEDGER_ID)?;
    let recovered = writer.entries().len();
    let governance = Governance::with_audit(pdp()?, writer);

    let durable = DurablePicture::open(state_dir.join("picture.sealed"), identity.data_sealer()?)
        .map_err(|e| format!("the picture does not open: {e:?}"))?;
    let state = PictureState::recover(durable);
    let entities = state.picture().entity_count();
    state_ready.set(true);

    let resource = |id: &str| Resource {
        id: ResourceId(id.into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose(purpose.to_string())].into_iter().collect(),
    };
    let feed_service = FeedService::new(
        state,
        governance,
        Arc::new(RwLock::new(fips_provider())),
        resource("heimdall-feed"),
        Purpose(purpose.to_string()),
        Arc::new(unix_seconds),
        metrics.clone(),
    );
    let channel_pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(pdp()?);
    let gateway = Gateway::new(
        Arc::new(fips_provider()),
        identity.cert.clone(),
        identity.signing_key.clone(),
        identity.anchor.clone(),
        channel_pdp,
        resource("heimdall-feed-channel"),
        Arc::new(feed_service.router()),
    );

    let listener = TcpListener::bind(args.get("bind").unwrap_or("0.0.0.0:7730"))
        .map_err(|e| format!("cannot bind the mesh port: {e}"))?;
    let mesh_addr = listener
        .local_addr()
        .map_err(|e| format!("the mesh port has no address: {e}"))?;
    mesh_ready.set(true);
    logger.info(
        "serving",
        &[
            ("mesh", &mesh_addr.to_string()),
            ("health", &health_addr.to_string()),
            ("roles", &identity.roles().join(",")),
            ("recovered_audit_entries", &recovered.to_string()),
            ("entities", &entities.to_string()),
        ],
    );
    serve_mesh(service, listener, gateway, metrics)
}
