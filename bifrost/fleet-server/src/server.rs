//! The `bifrost-server` process — and `bifrost-fleet`, its earlier name, kept as an
//! alias: the fleet manager plus the artefact registry, as one mesh service.
//! [FR-B-1] [FR-B-3] [FR-B-4] [RC-12] [RC-17] [RC-33] [NFR-SEC-1]
//!
//! ```text
//! bifrost-server --identity-dir DIR --state-dir DIR
//!                [--service NAME] [--purpose P] [--stale-after SECONDS]
//!                [--bind ADDR] [--health-bind ADDR]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME defaults
//! to the program's name, `bifrost-server` or `bifrost-fleet`). `--state-dir` holds
//! `policy.json` — the rules for `OpenChannel`, `CheckIn`, `ReadFleet`,
//! `RegisterArtefact` and `ReadArtefacts` — and `channels.json`, each channel's desired
//! version; the release signers the registry trusts, as `trust/release-signers/*.pub`
//! (none: every upload is refused); and the service's sealed `fleet.journal`,
//! `fleet.ledger` and `registry/`. It is locked while the service runs. Requests are
//! audited under `--purpose` (default `fleet-management`), the one purpose the channel
//! opens for. A node silent for `--stale-after` seconds (default 300) is reported stale.
//! PQ-mTLS is served on `--bind` (default `0.0.0.0:7740`); `/healthz`, `/readyz` and
//! `/metrics` on `--health-bind` (default `0.0.0.0:9090`).
//!
//! Both names serve the same state: the journal, ledger and registry ids do not depend
//! on the program's name, so `bifrost-fleet`'s state directory opens under
//! `bifrost-server` unchanged.

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;

use crate::registry::{load_signers, Registry};
use crate::{parse_channels, FleetService};
use bifrost_fleet::DurableFleet;
use gateway::Gateway;
use governance::Governance;
use policy::{
    ClassificationLevel, DefaultPdp, Marking, PolicyDecisionPoint, Purpose, Resource, ResourceId,
};
use pqc_agility::backend::fips::provider as fips_provider;
use service_runtime::policy_file::parse_policy;
use service_runtime::{
    serve_mesh, spawn_health_port, unix_seconds, Args, DirLock, ServiceIdentity,
};
use telemetry::{HealthRegistry, Logger, MetricsRegistry, StdoutJsonSink, Toggle};

const USAGE: &str = "--identity-dir DIR --state-dir DIR [--service NAME] \
                     [--purpose P] [--stale-after SECONDS] [--bind ADDR] [--health-bind ADDR]";

/// The id of the service's audit ledger.
const LEDGER_ID: &str = "bifrost-fleet-audit";
/// The id the fleet journal is sealed under.
const FLEET_ID: &str = "bifrost-fleet";

/// Run the service as `program` (its name in messages and its default `--service`).
pub fn main(program: &str) {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(
        &argv,
        &[
            "identity-dir",
            "state-dir",
            "service",
            "purpose",
            "stale-after",
            "bind",
            "health-bind",
        ],
        &[],
    ) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{program}: {e}\nusage: {program} {USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(program, &args) {
        eprintln!("{program}: {e}");
        std::process::exit(1);
    }
}

fn run(program: &str, args: &Args) -> Result<(), String> {
    let service = args.get("service").unwrap_or(program);
    let identity_dir = args.require("identity-dir")?;
    let state_dir = Path::new(args.require("state-dir")?);
    let purpose = args.get("purpose").unwrap_or("fleet-management");
    let stale_after = match args.get("stale-after") {
        Some(s) => s
            .parse::<u64>()
            .map_err(|_| format!("--stale-after must be a whole number of seconds, not '{s}'"))?,
        None => 300,
    };
    let logger = Logger::new(service, &StdoutJsonSink);

    let state_ready = Arc::new(Toggle::new("fleet-state", false));
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

    let _lock = DirLock::acquire(state_dir, "fleet.lock", service)?;
    let identity = ServiceIdentity::load(identity_dir, service)?;

    let read = |name: &str| {
        let path = state_dir.join(name);
        std::fs::read_to_string(&path)
            .map(|text| (text, path.display().to_string()))
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
    };
    let (policy_text, policy_path) = read("policy.json")?;
    let pdp = || -> Result<DefaultPdp, String> {
        parse_policy(&policy_text)
            .map(DefaultPdp::new)
            .map_err(|e| format!("{policy_path}: {e}"))
    };
    let (channels_text, channels_path) = read("channels.json")?;
    let channels = parse_channels(&channels_text).map_err(|e| format!("{channels_path}: {e}"))?;

    let writer = identity.audit_writer(state_dir.join("fleet.ledger"), LEDGER_ID)?;
    let recovered = writer.entries().len();
    let governance = Governance::with_audit(pdp()?, writer);

    let mut fleet = DurableFleet::open(
        state_dir.join("fleet.journal"),
        identity.data_sealer()?,
        FLEET_ID,
    )
    .map_err(|e| format!("the fleet journal does not open: {e}"))?;
    for (name, desired) in channels {
        if fleet.fleet().channel_desired(&name) != Some(&desired) {
            fleet
                .set_channel(name, desired)
                .map_err(|e| format!("cannot record a channel: {e}"))?;
        }
    }
    let installations = fleet.fleet().installations().count();

    let signers = load_signers(&state_dir.join("trust").join("release-signers"))?;
    let registry = Registry::open(state_dir.join("registry"), identity.data_sealer()?, signers)
        .map_err(|e| format!("the artefact registry does not open: {e}"))?;
    let signer_ids = registry.signer_ids().join(",");
    let artefacts = registry.records().count();
    let rejected = registry.rejections().len();
    state_ready.set(true);

    let resource = |id: &str| Resource {
        id: ResourceId(id.into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose(purpose.to_string())].into_iter().collect(),
    };
    let fleet_service = FleetService::new(
        fleet,
        registry,
        governance,
        resource("bifrost-fleet"),
        Purpose(purpose.to_string()),
        stale_after,
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
        resource("bifrost-fleet-channel"),
        Arc::new(fleet_service.router()),
    );

    let listener = TcpListener::bind(args.get("bind").unwrap_or("0.0.0.0:7740"))
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
            ("installations", &installations.to_string()),
            ("artefacts", &artefacts.to_string()),
            ("rejected", &rejected.to_string()),
            ("release_signers", &signer_ids),
        ],
    );
    serve_mesh(service, listener, gateway, metrics)
}
