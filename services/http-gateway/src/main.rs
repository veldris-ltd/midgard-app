//! http-gateway binary — the browser-facing HTTP/JSON edge for the console UIs.
//!
//! Usage:
//!   http-gateway [ADDR]
//!       Development mode (default 127.0.0.1:7710): an illustrative data set seeded
//!       into an in-memory fabric, one built-in login, an ephemeral issuer key.
//!   http-gateway --state-dir DIR [--bind ADDR] [--allow-origin URL]... [--token-ttl SECONDS]
//!                [--mesh-bind ADDR --mesh-identity-dir DIR [--mesh-service NAME]
//!                 [--mesh-purpose P]] [--heimdall-addr HOST:PORT --heimdall-identity-dir DIR
//!                [--heimdall-service NAME] [--heimdall-purpose P]]
//!       Serve the enclosure in DIR, created by `mg-admin init`: persisted keys, the
//!       durable store, ledger and identity directory, and DIR/policy.json. Nothing
//!       is seeded and there is no built-in login. Each --allow-origin adds a browser
//!       origin to the CORS allowlist, which is otherwise empty. --token-ttl sets how
//!       long a login token lasts, 60 to 86400 seconds (default 3600).
//!
//!       With --mesh-bind, the gateway also serves `SyncRecords` over PQ-mTLS, so a
//!       certified workload — the Forge worker — can sync objects and links into the
//!       fabric. It presents the identity `mg-admin mesh issue --service NAME` wrote to
//!       --mesh-identity-dir (NAME defaults to `http-gateway`); policy.json decides
//!       who may open the channel and `SyncRecords`, under --mesh-purpose (default
//!       `data-integration`).
//!
//!       With --heimdall-addr, signed-in users the policy lets `Read` it get Heimdall's
//!       common operating picture at GET /api/v1/heimdall/picture, and its track
//!       history at GET /api/v1/heimdall/history, and its alerts at
//!       GET /api/v1/heimdall/alerts; users it lets `Apply` acknowledge an alert with
//!       POST /api/v1/heimdall/alerts/{id}/ack. The gateway calls `heimdall-feed` over
//!       PQ-mTLS as the identity in --heimdall-identity-dir (service
//!       --heimdall-service, default `picture-analyst`; Heimdall's policy must grant it
//!       ReadPicture, ReadHistory, ReadAlerts and AcknowledgeAlert), declaring
//!       --heimdall-purpose (default `situational-awareness`). [FR-H-4]
//!
//! Point a console at it with `VITE_GATEWAY=http` (see products/*/ui). [RC-24] [RC-29]
//! [FR-F-4] [NFR-SEC-1]

use std::net::SocketAddr;
use std::process::exit;
use std::sync::Arc;

use http_gateway::boot::{open_state_dir, DurableGateway};
use http_gateway::heimdall_bridge::HeimdallBridge;
use http_gateway::mesh_sync::spawn_mesh_listener;
use http_gateway::policy_file::parse_policy;
use http_gateway::state_dir::StateDir;
use policy::DefaultPdp;
use service_runtime::ServiceIdentity;

const DEFAULT_BIND: &str = "127.0.0.1:7710";

const USAGE: &str = "\
usage:
  http-gateway [ADDR]
  http-gateway --state-dir DIR [--bind ADDR] [--allow-origin URL]... [--token-ttl SECONDS]
               [--mesh-bind ADDR --mesh-identity-dir DIR [--mesh-service NAME] [--mesh-purpose P]]
               [--heimdall-addr HOST:PORT --heimdall-identity-dir DIR [--heimdall-service NAME]
               [--mimir-observe-addr HOST:PORT --mimir-identity-dir DIR [--mimir-service NAME]]
               [--analytics-addr HOST:PORT --analytics-token-file FILE]
               [--forge-jobs FILE [--forge-status FILE] [--heimdall-feeds-status FILE]]
                [--heimdall-purpose P]]
  --token-ttl SECONDS  login token lifetime, 60..=86400 (default 3600)";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }

    // A bare address (or nothing) is development mode, as it always has been.
    if argv.first().map_or(true, |a| !a.starts_with("--")) {
        if argv.len() > 1 {
            fail_usage("development mode takes at most one argument, the bind address");
        }
        let addr = argv.first().map_or(DEFAULT_BIND, String::as_str);
        if let Err(e) = http_gateway::serve(addr) {
            eprintln!("http-gateway: failed to bind {addr}: {e}");
            exit(1);
        }
        return;
    }

    let mut state_dir = None;
    let mut bind = DEFAULT_BIND.to_string();
    let mut origins = Vec::new();
    let mut mesh_bind = None;
    let mut mesh_identity_dir = None;
    let mut mesh_service = "http-gateway".to_string();
    let mut mesh_purpose = "data-integration".to_string();
    let mut heimdall_addr = None;
    let mut heimdall_identity_dir = None;
    let mut heimdall_service = "picture-analyst".to_string();
    let mut mimir_addr = None;
    let mut mimir_identity_dir = None;
    let mut mimir_service = "mimir-observer".to_string();
    let mut heimdall_purpose = "situational-awareness".to_string();
    let mut analytics_addr = None;
    let mut analytics_token_file = None;
    let mut forge_jobs = None;
    let mut forge_status = None;
    let mut feeds_status = None;
    let mut token_ttl = http_gateway::DEFAULT_TOKEN_TTL;
    let mut args = argv.iter();
    while let Some(flag) = args.next() {
        let Some(value) = args.next().filter(|v| !v.starts_with("--")) else {
            fail_usage(&format!("{flag} requires a value"));
        };
        match flag.as_str() {
            "--state-dir" if state_dir.is_none() => state_dir = Some(value.clone()),
            "--bind" => bind = value.clone(),
            "--allow-origin" => origins.push(value.clone()),
            "--mesh-bind" => mesh_bind = Some(value.clone()),
            "--mesh-identity-dir" => mesh_identity_dir = Some(value.clone()),
            "--mesh-service" => mesh_service = value.clone(),
            "--mesh-purpose" => mesh_purpose = value.clone(),
            "--heimdall-addr" => heimdall_addr = Some(value.clone()),
            "--heimdall-identity-dir" => heimdall_identity_dir = Some(value.clone()),
            "--heimdall-service" => heimdall_service = value.clone(),
            "--mimir-observe-addr" => mimir_addr = Some(value.clone()),
            "--mimir-identity-dir" => mimir_identity_dir = Some(value.clone()),
            "--mimir-service" => mimir_service = value.clone(),
            "--heimdall-purpose" => heimdall_purpose = value.clone(),
            "--analytics-addr" => analytics_addr = Some(value.clone()),
            "--analytics-token-file" => analytics_token_file = Some(value.clone()),
            "--forge-jobs" => forge_jobs = Some(value.clone()),
            "--forge-status" => forge_status = Some(value.clone()),
            "--heimdall-feeds-status" => feeds_status = Some(value.clone()),
            "--token-ttl" => {
                token_ttl = http_gateway::parse_token_ttl(value).unwrap_or_else(|e| fail_usage(&e))
            }
            "--state-dir" => fail_usage("--state-dir given twice"),
            other => fail_usage(&format!("unknown option {other}")),
        }
    }
    let Some(state_dir) = state_dir else {
        fail_usage("--state-dir is required with options");
    };
    if mesh_bind.is_some() != mesh_identity_dir.is_some() {
        fail_usage("--mesh-bind and --mesh-identity-dir go together");
    }
    if heimdall_addr.is_some() != heimdall_identity_dir.is_some() {
        fail_usage("--heimdall-addr and --heimdall-identity-dir go together");
    }
    if mimir_addr.is_some() != mimir_identity_dir.is_some() {
        fail_usage("--mimir-observe-addr and --mimir-identity-dir go together");
    }
    if forge_jobs.is_none() && (forge_status.is_some() || feeds_status.is_some()) {
        fail_usage("--forge-status and --heimdall-feeds-status need --forge-jobs");
    }
    if analytics_addr.is_some() != analytics_token_file.is_some() {
        fail_usage("--analytics-addr and --analytics-token-file go together");
    }

    let served = match open_state_dir(&StateDir::new(&state_dir), origins) {
        Ok(served) => served,
        Err(e) => {
            eprintln!("http-gateway: cannot serve {state_dir}: {e}");
            exit(1);
        }
    };
    eprintln!(
        "http-gateway: serving the enclosure in {state_dir} — {} principal(s) enrolled, {} \
         administrator(s), {} object type(s) and {} action(s) registered, {} audit entries \
         recovered and re-verified",
        served.principals,
        served.administrators,
        served.object_types,
        served.actions,
        served.audit_entries
    );
    if served.principals == 0 {
        eprintln!(
            "http-gateway: no principals are enrolled, so nobody can log in; stop the gateway \
             and run `mg-admin user add --state-dir {state_dir} ...`"
        );
    }
    // The lock lives as long as the process serves: no second gateway or mg-admin can
    // write this directory meanwhile.
    let (gateway, _lock) = served.into_parts();
    let mut gateway = gateway.with_token_ttl(token_ttl);
    if let (Some(addr), Some(dir)) = (heimdall_addr, heimdall_identity_dir) {
        let identity = match ServiceIdentity::load(&dir, &heimdall_service) {
            Ok(identity) => identity,
            Err(e) => {
                eprintln!("http-gateway: cannot load the Heimdall reader identity: {e}");
                exit(1);
            }
        };
        gateway = gateway.with_heimdall(HeimdallBridge::new(identity, &addr, &heimdall_purpose));
        eprintln!("http-gateway: serving Heimdall's picture from {addr} as {heimdall_service}");
    }
    if let (Some(addr), Some(dir)) = (mimir_addr, mimir_identity_dir) {
        let identity = match ServiceIdentity::load(&dir, &mimir_service) {
            Ok(identity) => identity,
            Err(e) => {
                eprintln!("http-gateway: cannot load the Mimir observer identity: {e}");
                exit(1);
            }
        };
        gateway = gateway.with_mimir(http_gateway::mimir_observer::MimirObserver::new(
            identity,
            &addr,
            "investigation",
        ));
        eprintln!(
            "http-gateway: serving Mimir's model plane and sessions from {addr} as {mimir_service}"
        );
    }
    if let Some(jobs) = forge_jobs {
        eprintln!("http-gateway: serving Forge's live view from {jobs}");
        gateway = gateway.with_forge(http_gateway::forge_view::ForgeView::new(
            jobs.into(),
            forge_status.map(Into::into),
            feeds_status.map(Into::into),
        ));
    }
    if let (Some(addr), Some(file)) = (analytics_addr, analytics_token_file) {
        let token = match std::fs::read_to_string(&file) {
            Ok(t) if !t.trim().is_empty() => t,
            _ => {
                eprintln!("http-gateway: cannot read the camera analytics token from {file}");
                exit(1);
            }
        };
        gateway = gateway.with_analytics(http_gateway::camera_analytics::AnalyticsClient::new(
            &addr, &token,
        ));
        eprintln!("http-gateway: camera analytics on request via {addr}");
    }
    let gateway = Arc::new(gateway);
    start_upkeep(gateway.clone());

    if let (Some(mesh_bind), Some(identity_dir)) = (mesh_bind, mesh_identity_dir) {
        match start_mesh(
            &gateway,
            &state_dir,
            &identity_dir,
            &mesh_service,
            &mesh_purpose,
            &mesh_bind,
        ) {
            Ok(addr) => eprintln!(
                "http-gateway: serving SyncRecords over PQ-mTLS on {addr} as {mesh_service}"
            ),
            Err(e) => {
                eprintln!("http-gateway: cannot serve the mesh: {e}");
                exit(1);
            }
        }
    }

    if let Err(e) = http_gateway::serve_shared(&bind, gateway) {
        eprintln!("http-gateway: failed to bind {bind}: {e}");
        exit(1);
    }
}

fn start_mesh(
    gateway: &Arc<DurableGateway>,
    state_dir: &str,
    identity_dir: &str,
    service: &str,
    purpose: &str,
    bind: &str,
) -> Result<SocketAddr, String> {
    let identity = ServiceIdentity::load(identity_dir, service)?;
    let policy_path = StateDir::new(state_dir).policy();
    let text = std::fs::read_to_string(&policy_path)
        .map_err(|e| format!("cannot read {}: {e}", policy_path.display()))?;
    let policy = parse_policy(&text).map_err(|e| format!("{}: {e}", policy_path.display()))?;
    spawn_mesh_listener(
        gateway.clone(),
        &identity,
        DefaultPdp::new(policy),
        bind,
        purpose,
    )
    .map_err(|e| format!("cannot bind {bind}: {e}"))
}

/// Store upkeep once a minute: the ontology's retention windows (each sweep audited; a
/// backlog larger than one sweep's limit clears over the following ones), then journal
/// compaction when the journal has outgrown its data. [SAD_00 §8.5]
fn start_upkeep(gateway: Arc<DurableGateway>) {
    if gateway.has_retention() {
        eprintln!("http-gateway: enforcing the ontology's retention windows every 60 s");
    }
    std::thread::spawn(move || loop {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        match gateway.enforce_retention(now) {
            Ok(swept) => {
                for s in swept {
                    eprintln!(
                        "http-gateway: retention removed {} {}{}",
                        s.removed,
                        s.object_type,
                        if s.more { " (more to follow)" } else { "" }
                    );
                }
            }
            Err(e) => eprintln!("http-gateway: retention sweep failed: {e}"),
        }
        match gateway.compact_store_if_due(now) {
            Ok(Some((before, after))) => eprintln!(
                "http-gateway: compacted the object store journal from {before} to {after} records"
            ),
            Ok(None) => {}
            Err(e) => eprintln!("http-gateway: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_secs(60));
    });
}

fn fail_usage(message: &str) -> ! {
    eprintln!("http-gateway: {message}\n{USAGE}");
    exit(2);
}
