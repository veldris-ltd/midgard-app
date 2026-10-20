//! `heimdall-feed` — the Heimdall feed gateway as a running service. [FR-H-1] [FR-H-3]
//! [NFR-PQ-3] [RC-13] [RC-33] [NFR-SEC-1]
//!
//! ```text
//! heimdall-feed --identity-dir DIR --state-dir DIR
//!               [--service NAME] [--purpose P] [--bind ADDR] [--health-bind ADDR]
//!               [--track-ttl SECONDS] [--history-retention SECONDS]
//!               [--history-page-bytes BYTES] [--picture-page-bytes BYTES]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME defaults
//! to `heimdall-feed`). `--state-dir` holds `policy.json` — the rules for `OpenChannel`,
//! `Ingest`, `ReadPicture`, `ReadHistory`, `ReadAlerts` and `AcknowledgeAlert` — and the service's sealed
//! `picture.sealed`, `history/` and `feed.ledger`; it is locked while the service runs. Requests are audited under
//! `--purpose` (default `situational-awareness`), the one purpose the channel opens
//! for. PQ-mTLS is served on `--bind` (default `0.0.0.0:7730`); `/healthz`, `/readyz`
//! and `/metrics` on `--health-bind` (default `0.0.0.0:9090`).
//!
//! Entities are classified by `rules.json` in the state directory when it exists (see
//! [`heimdall_feed_server::rules_file`]), otherwise by the built-in default rules.
//! `--track-ttl` removes entities not seen for that many seconds (default 0: never).
//! Track history (each positioned observation's point, sealed in `history/`) is kept
//! for `--history-retention` seconds (default 86400) and read with `ReadHistory`, in
//! pages of at most `--history-page-bytes` (default 921600; smaller only to exercise
//! paging). `ReadPicture` serves the picture in pages of at most
//! `--picture-page-bytes` of entities (default 921600; smaller only to exercise paging).
//! [FR-H-1] [RC-13]
//!
//! `ttl.json` in the state directory, when it exists, sets the time-to-live of exact
//! kinds (see [`heimdall_feed_server::ttl_file`]); other kinds use `--track-ttl`.
//! `alerts.json`, when it exists, configures the alerts raised (see
//! [`heimdall_feed_server::alerts`]); they are sealed in `alerts.sealed`, and read and
//! acknowledged with `ReadAlerts` and `AcknowledgeAlert`. [FR-H-2] [FR-H-4]

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, RwLock};

use gateway::Gateway;
use governance::Governance;
use heimdall_classify::RulesClassifier;
use heimdall_feed_server::{
    describe_ttl, parse_alerts, parse_rules, parse_ttl, AlertBook, AlertConfig, FeedService,
    PictureState, DEFAULT_HISTORY_RETENTION, HISTORY_PAGE_BYTES, PICTURE_PAGE_BYTES,
};
use heimdall_fusion::FusionEngine;
use heimdall_store::{DurableHistory, DurablePicture};
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
                     [--purpose P] [--bind ADDR] [--health-bind ADDR] [--track-ttl SECONDS] \
                     [--history-retention SECONDS] [--history-page-bytes BYTES] \
                     [--picture-page-bytes BYTES] \
                     [--mimir-addr HOST:PORT --mimir-identity-dir DIR [--mimir-service NAME]]";

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
            "track-ttl",
            "history-retention",
            "history-page-bytes",
            "picture-page-bytes",
            "mimir-addr",
            "mimir-identity-dir",
            "mimir-service",
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
    let track_ttl = match args.get("track-ttl") {
        Some(s) => s
            .parse::<u64>()
            .map_err(|_| format!("--track-ttl must be a whole number of seconds, not '{s}'"))?,
        None => 0,
    };
    let history_retention = match args.get("history-retention") {
        Some(s) => s.parse::<u64>().ok().filter(|v| *v > 0).ok_or_else(|| {
            format!("--history-retention must be a positive number of seconds, not '{s}'")
        })?,
        None => DEFAULT_HISTORY_RETENTION,
    };
    let history_page_bytes = match args.get("history-page-bytes") {
        Some(s) => s
            .parse::<usize>()
            .ok()
            .filter(|v| (1..=HISTORY_PAGE_BYTES).contains(v))
            .ok_or_else(|| {
                format!("--history-page-bytes must be 1..={HISTORY_PAGE_BYTES}, not '{s}'")
            })?,
        None => HISTORY_PAGE_BYTES,
    };
    let picture_page_bytes = match args.get("picture-page-bytes") {
        Some(s) => s
            .parse::<usize>()
            .ok()
            .filter(|v| (1..=PICTURE_PAGE_BYTES).contains(v))
            .ok_or_else(|| {
                format!("--picture-page-bytes must be 1..={PICTURE_PAGE_BYTES}, not '{s}'")
            })?,
        None => PICTURE_PAGE_BYTES,
    };
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
    let rules_path = state_dir.join("rules.json");
    let (classifier, rules_source) = if rules_path.exists() {
        let text = std::fs::read_to_string(&rules_path)
            .map_err(|e| format!("cannot read {}: {e}", rules_path.display()))?;
        let rules = parse_rules(&text).map_err(|e| format!("{}: {e}", rules_path.display()))?;
        let source = format!("rules.json ({} rules)", rules.rules().len());
        (rules, source)
    } else {
        (
            RulesClassifier::with_default_rules(),
            "default rules".to_string(),
        )
    };
    let read_optional = |name: &str| -> Result<Option<String>, String> {
        let path = state_dir.join(name);
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&path)
            .map(Some)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
    };
    let kind_ttls = match read_optional("ttl.json")? {
        Some(text) => parse_ttl(&text).map_err(|e| format!("ttl.json: {e}"))?,
        None => Default::default(),
    };
    let alert_config = match read_optional("alerts.json")? {
        Some(text) => parse_alerts(&text).map_err(|e| format!("alerts.json: {e}"))?,
        None => AlertConfig::default(),
    };
    let alerts_source = if state_dir.join("alerts.json").exists() {
        format!("alerts.json ({})", alert_config.describe())
    } else {
        "none".to_string()
    };
    let alert_book = AlertBook::open(
        state_dir.join("alerts.sealed"),
        identity.data_sealer()?,
        alert_config,
    )?;
    let open_alerts = alert_book.newest_first(true).count();
    let history = DurableHistory::open(state_dir.join("history"), identity.data_sealer()?)
        .map_err(|e| format!("the track history does not open: {e}"))?;
    let history_points = history.point_count();
    let state = PictureState::recover_with(
        durable,
        FusionEngine::new().with_classifier(Box::new(classifier)),
    )
    .with_history(history)
    .with_alerts(alert_book);
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
    feed_service.set_track_ttl(track_ttl);
    let kind_ttl_text = describe_ttl(&kind_ttls);
    feed_service.set_kind_ttls(kind_ttls);
    feed_service.set_history_retention(history_retention);
    feed_service.set_history_page_bytes(history_page_bytes);
    feed_service.set_picture_page_bytes(picture_page_bytes);
    // Decision support through Mimir, when configured: its own consumer identity
    // (role inference-consumer), separate from this service's. [FR-H-4] [FR-MI-8]
    let advisor = match (args.get("mimir-addr"), args.get("mimir-identity-dir")) {
        (Some(addr), Some(dir)) => {
            let who = args.get("mimir-service").unwrap_or("heimdall-advisor");
            let mimir = ServiceIdentity::load(dir, who)?;
            let signing_key = Arc::try_unwrap(mimir.signing_key)
                .map_err(|_| "the Mimir identity key is shared".to_string())?;
            let client = mimir_bridge::PqMtlsClient::new(mimir_bridge::MimirEndpoint {
                addr: addr.to_string(),
                anchor: mimir.anchor,
                cert: mimir.cert,
                signing_key,
                purpose: "investigation".into(),
                posture: pq_mtls::Posture {
                    level: ClassificationLevel::UNCLASSIFIED,
                    compartments: BTreeSet::new(),
                },
            });
            feed_service.set_advisor(Arc::new(
                heimdall_decision_support::MimirOptionProvider::new(client).with_max_tokens(700),
            ));
            format!("{addr} as {who}")
        }
        (None, None) => "off".to_string(),
        _ => return Err("--mimir-addr and --mimir-identity-dir go together".into()),
    };
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
            ("classifier", &rules_source),
            ("track_ttl", &track_ttl.to_string()),
            ("kind_ttls", &kind_ttl_text),
            ("alerts", &alerts_source),
            ("open_alerts", &open_alerts.to_string()),
            ("history_points", &history_points.to_string()),
            ("decision_support", &advisor),
            ("history_retention", &history_retention.to_string()),
        ],
    );
    serve_mesh(service, listener, gateway, metrics)
}
