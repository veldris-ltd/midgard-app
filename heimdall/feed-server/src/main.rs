//! `heimdall-feed` — the Heimdall feed gateway as a running service. [FR-H-1] [FR-H-3]
//! [NFR-PQ-3] [RC-13] [RC-33] [NFR-SEC-1]
//!
//! ```text
//! heimdall-feed --identity-dir DIR --state-dir DIR
//!               [--service NAME] [--purpose P] [--bind ADDR] [--health-bind ADDR]
//!               [--max-connections N] [--idle-timeout SECONDS]
//!               [--rate-burst N] [--rate-per-sec N] [--requests-per-conn N]
//!               [--max-entities N] [--max-entities-per-feed N] [--max-attrs N]
//!               [--max-attr-bytes N] [--evict-after SECONDS]
//!               [--max-observation-skew S] [--max-observation-age S] [--stale-after S]
//!               [--resource-marking BAND[:COMPARTMENT,...]]
//! heimdall-feed --identity-dir DIR --state-dir DIR --compact [--service NAME]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME defaults
//! to `heimdall-feed`). `--state-dir` holds `policy.json` — the rules for `OpenChannel`,
//! `Ingest` and `ReadPicture` — and the service's sealed picture (the snapshot
//! `picture.sealed` and the journal of changes since, `picture.journal`; back up both)
//! and `feed.ledger`; it is locked while the service runs. Requests are audited under
//! `--purpose` (default `situational-awareness`), the one purpose the channel opens
//! for. PQ-mTLS is served on `--bind` (default `0.0.0.0:7730`); `/healthz`, `/readyz`
//! and `/metrics` on `--health-bind` (default `0.0.0.0:9090`).
//!
//! The mesh listener serves at most `--max-connections` connections at once (default
//! 128), and closes one that sends nothing for `--idle-timeout` seconds (default 30).
//! Each certified peer may make `--rate-burst` requests in a burst (default 100),
//! refilled at `--rate-per-sec` a second (default 20; a burst and a rate of 0 turn the
//! limit off), and a connection is closed after `--requests-per-conn` requests (default
//! 10000; 0 is unlimited). A value that is not a number is a usage error.
//! [VLD-ANB-MIDGARD-0004] These are the Ingest quota: every submission is a request
//! from the source's identity, so one source cannot flood the picture.
//!
//! The picture holds at most `--max-entities` entities (default 50000), and at most
//! `--max-entities-per-feed` (default 10000) that one feed has contributed to. An
//! observation may carry `--max-attrs` attributes (default 64) of at most
//! `--max-attr-bytes` bytes each (default 4096); one with more is malformed. A new
//! entity past a cap first evicts entities not seen for `--evict-after` seconds
//! (default 604800, 7 days); if none is that old it is refused as `picture-full` and
//! audited. [VLD-ANB-MIDGARD-0017]
//!
//! `--compact` folds `picture.journal` into `picture.sealed` and exits, so the snapshot
//! alone holds the picture: run it, with the service stopped, before rolling back to a
//! build that reads only `picture.sealed`.
//!
//! An observation is fused only if its `observed_at` is at most
//! `--max-observation-skew` seconds ahead of the service's clock (default 300) and at
//! most `--max-observation-age` seconds behind it (default 604800, seven days); any
//! other is refused and audited. The picture marks an entity stale once nothing has
//! been received for it for `--stale-after` seconds (default 900).
//! [VLD-ANB-MIDGARD-0015]
//!
//! Each feed's observations carry a monotonic sequence; one not above the feed's last
//! admitted sequence is refused as a replay. The last sequence per feed is sealed in
//! `--state-dir`'s `feed-sequences.sealed`, so a restart keeps refusing replays.
//! [VLD-ANB-MIDGARD-0041]
//!
//! The service's resources are marked `--resource-marking` (a band, optionally with
//! compartments; default `open`): a peer is authorised against it at the clearance its
//! certificate grants. [VLD-ANB-MIDGARD-0063]

use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, RwLock};

use gateway::Gateway;
use governance::Governance;
use heimdall_feed_server::{FeedService, IngestLimits, PictureState, SequenceFile};
use heimdall_picture::PictureLimits;
use heimdall_store::DurablePicture;
use policy::{DefaultPdp, PolicyDecisionPoint, Purpose, Resource, ResourceId};
use pqc_agility::backend::fips::provider as fips_provider;
use service_runtime::marking::{open_marking, parse_marking};
use service_runtime::policy_file::parse_policy;
use service_runtime::{
    apply_mesh_limits, mesh_limits_from_args, serve_mesh_with, spawn_health_port, unix_seconds,
    Args, DirLock, MeshLimits, ServiceIdentity,
};
use telemetry::{HealthRegistry, Logger, MetricsRegistry, StdoutJsonSink, Toggle};

const USAGE: &str = "usage: heimdall-feed --identity-dir DIR [--keystore-passphrase-file PATH] \
                     --state-dir DIR [--service NAME] \
                     [--purpose P] [--resource-marking BAND[:COMPARTMENT,...]] [--bind ADDR] \
                     [--health-bind ADDR] \
                     [--max-connections N] [--idle-timeout SECONDS] [--rate-burst N] \
                     [--rate-per-sec N] [--requests-per-conn N] [--max-entities N] \
                     [--max-entities-per-feed N] [--max-attrs N] [--max-attr-bytes N] \
                     [--evict-after SECONDS] [--max-observation-skew S] \
                     [--max-observation-age S] [--stale-after S]\n       \
                     heimdall-feed --identity-dir DIR [--keystore-passphrase-file PATH] \
                     --state-dir DIR --compact [--service NAME]";

/// The id of the service's audit ledger.
const LEDGER_ID: &str = "heimdall-feed-audit";

fn main() {
    service_runtime::harden_process("heimdall-feed");
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(
        &argv,
        &[
            "identity-dir",
            "keystore-passphrase-file",
            "state-dir",
            "service",
            "purpose",
            "resource-marking",
            "bind",
            "health-bind",
            "max-connections",
            "idle-timeout",
            "rate-burst",
            "rate-per-sec",
            "requests-per-conn",
            "max-entities",
            "max-entities-per-feed",
            "max-attrs",
            "max-attr-bytes",
            "evict-after",
            "max-observation-skew",
            "max-observation-age",
            "stale-after",
        ],
        &["compact"],
    ) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("heimdall-feed: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let limits = match mesh_limits_from_args(&args) {
        Ok(limits) => limits,
        Err(e) => {
            eprintln!("heimdall-feed: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let picture_limits = match picture_limits_from_args(&args) {
        Ok(limits) => limits,
        Err(e) => {
            eprintln!("heimdall-feed: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let result = if args.flag("compact") {
        compact(&args)
    } else {
        run(&args, &limits, picture_limits)
    };
    if let Err(e) = result {
        eprintln!("heimdall-feed: {e}");
        std::process::exit(1);
    }
}

/// [`PictureLimits::default`] with what `--max-entities`, `--max-entities-per-feed`,
/// `--max-attrs`, `--max-attr-bytes` and `--evict-after` say instead. A cap of 0 is
/// refused: it would refuse every observation.
fn picture_limits_from_args(args: &Args) -> Result<PictureLimits, String> {
    let whole = |key: &str| -> Result<Option<u64>, String> {
        args.get(key)
            .map(|v| {
                v.parse::<u64>()
                    .map_err(|_| format!("--{key} must be a whole number, not '{v}'"))
            })
            .transpose()
    };
    let cap = |key: &str, default: usize| -> Result<usize, String> {
        match whole(key)? {
            None => Ok(default),
            Some(0) => Err(format!("--{key} must be at least 1")),
            Some(n) => Ok(usize::try_from(n).unwrap_or(usize::MAX)),
        }
    };
    let defaults = PictureLimits::default();
    Ok(PictureLimits {
        max_entities: cap("max-entities", defaults.max_entities)?,
        max_entities_per_feed: cap("max-entities-per-feed", defaults.max_entities_per_feed)?,
        max_attrs_per_entity: cap("max-attrs", defaults.max_attrs_per_entity)?,
        max_attr_bytes: cap("max-attr-bytes", defaults.max_attr_bytes)?,
        evict_after: whole("evict-after")?.unwrap_or(defaults.evict_after),
    })
}

/// `--compact`: fold `picture.journal` into `picture.sealed` and exit, with the state
/// directory locked, so the snapshot alone holds the whole picture (before a backup of
/// one file, or a rollback to a build that reads only the snapshot).
fn compact(args: &Args) -> Result<(), String> {
    let service = args.get("service").unwrap_or("heimdall-feed");
    let state_dir = Path::new(args.require("state-dir")?);
    let _lock = DirLock::acquire(state_dir, "feed.lock", service)?;
    let identity = ServiceIdentity::load(
        args.require("identity-dir")?,
        service,
        &service_runtime::RootWrap::Passphrase(&service_runtime::load_passphrase(
            args.get(service_runtime::PASSPHRASE_FLAG),
            std::env::var(service_runtime::PASSPHRASE_ENV).ok(),
        )?),
    )?;
    let mut durable =
        DurablePicture::open(state_dir.join("picture.sealed"), identity.data_sealer()?)
            .map_err(|e| format!("the picture does not open: {e:?}"))?;
    durable
        .compact()
        .map_err(|e| format!("the picture was not compacted: {e:?}"))?;
    println!(
        "compacted {} entities into {}",
        durable.picture().entity_count(),
        state_dir.join("picture.sealed").display()
    );
    Ok(())
}

fn run(args: &Args, limits: &MeshLimits, picture_limits: PictureLimits) -> Result<(), String> {
    let service = args.get("service").unwrap_or("heimdall-feed");
    let identity_dir = args.require("identity-dir")?;
    let state_dir = Path::new(args.require("state-dir")?);
    let purpose = args.get("purpose").unwrap_or("situational-awareness");
    let defaults = IngestLimits::default();
    let ingest_limits = IngestLimits {
        max_future_skew: seconds(args, "max-observation-skew", defaults.max_future_skew)?,
        max_age: seconds(args, "max-observation-age", defaults.max_age)?,
        stale_after: seconds(args, "stale-after", defaults.stale_after)?,
    };
    // What the service's resources are marked at: a channel and a request are authorised
    // against it with the peer's certified clearance. Default open. [VLD-ANB-MIDGARD-0063]
    let marking = match args.get("resource-marking") {
        Some(spec) => parse_marking(spec).map_err(|e| format!("--resource-marking: {e}"))?,
        None => open_marking(),
    };
    let logger = Logger::new(service, &StdoutJsonSink);

    let state_ready = Arc::new(Toggle::new("picture", false));
    let mesh_ready = Arc::new(Toggle::new("mesh-listener", false));
    let mut health = HealthRegistry::new();
    health.register(Box::new(state_ready.clone()));
    health.register(Box::new(mesh_ready.clone()));
    let metrics = Arc::new(MetricsRegistry::new());
    let health_bind = args.get("health-bind").unwrap_or("0.0.0.0:9090");
    if health_bind.starts_with("0.0.0.0") || health_bind.starts_with("[::]") {
        eprintln!(
            "heimdall-feed: WARNING health port binds every interface ({health_bind}); restrict \
             it with NetworkPolicy or a pod address. [VLD-ANB-MIDGARD-0027]"
        );
    }
    let health_addr = spawn_health_port(health_bind, Arc::new(health), metrics.clone())
        .map_err(|e| format!("cannot bind the health port: {e}"))?;

    let _lock = DirLock::acquire(state_dir, "feed.lock", service)?;
    let identity = ServiceIdentity::load(
        identity_dir,
        service,
        &service_runtime::RootWrap::Passphrase(&service_runtime::load_passphrase(
            args.get(service_runtime::PASSPHRASE_FLAG),
            std::env::var(service_runtime::PASSPHRASE_ENV).ok(),
        )?),
    )?;

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
    let (sequence_file, sequences) = SequenceFile::open(
        state_dir.join("feed-sequences.sealed"),
        identity.data_sealer()?,
    )
    .map_err(|e| format!("the feed sequences do not open: {e:?}"))?;
    let state = PictureState::recover(durable)
        .with_limits(picture_limits)
        .with_sequences(sequence_file, sequences);
    let entities = state.picture().entity_count();
    state_ready.set(true);

    let resource = |id: &str| Resource {
        id: ResourceId(id.into()),
        marking: marking.clone(),
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
        ingest_limits,
    );
    let channel_pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(pdp()?);
    let gateway = apply_mesh_limits(
        Gateway::new(
            Arc::new(fips_provider()),
            identity.cert.clone(),
            identity.signing_key.clone(),
            identity.anchor.clone(),
            channel_pdp,
            resource("heimdall-feed-channel"),
            Arc::new(feed_service.router()),
        ),
        limits,
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
            ("limits", &limits.to_string()),
            (
                "picture_limits",
                &format!(
                    "max-entities {}, max-entities-per-feed {}, max-attrs {}, \
                     max-attr-bytes {}, evict-after {}s",
                    picture_limits.max_entities,
                    picture_limits.max_entities_per_feed,
                    picture_limits.max_attrs_per_entity,
                    picture_limits.max_attr_bytes,
                    picture_limits.evict_after
                ),
            ),
            ("recovered_audit_entries", &recovered.to_string()),
            ("entities", &entities.to_string()),
        ],
    );
    serve_mesh_with(service, listener, gateway, metrics, limits.listener)
}

/// A whole number of seconds from `--flag`, or `default` when it is not given.
fn seconds(args: &Args, flag: &str, default: u64) -> Result<u64, String> {
    match args.get(flag) {
        Some(s) => s
            .parse::<u64>()
            .map_err(|_| format!("--{flag} must be a whole number of seconds, not '{s}'")),
        None => Ok(default),
    }
}
