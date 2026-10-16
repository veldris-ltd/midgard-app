//! http-gateway binary — the browser-facing HTTP/JSON edge for the console UIs.
//!
//! Usage:
//!   http-gateway [ADDR]
//!       Development mode (default 127.0.0.1:7710): an illustrative data set seeded
//!       into an in-memory fabric, one built-in login, an ephemeral issuer key.
//!   http-gateway --state-dir DIR [--bind ADDR] [--allow-origin URL]...
//!                [--mesh-bind ADDR --mesh-identity-dir DIR [--mesh-service NAME]
//!                 [--mesh-purpose P]] [--heimdall-addr HOST:PORT --heimdall-identity-dir DIR
//!                [--heimdall-service NAME] [--heimdall-purpose P]]
//!       Serve the enclosure in DIR, created by `mg-admin init`: persisted keys, the
//!       durable store, ledger and identity directory, and DIR/policy.json. Nothing
//!       is seeded and there is no built-in login. Each --allow-origin adds a browser
//!       origin to the CORS allowlist, which is otherwise empty.
//!
//!       With --mesh-bind, the gateway also serves `SyncRecords` over PQ-mTLS, so a
//!       certified workload — the Forge worker — can sync objects and links into the
//!       fabric. It presents the identity `mg-admin mesh issue --service NAME` wrote to
//!       --mesh-identity-dir (NAME defaults to `http-gateway`); policy.json decides
//!       who may open the channel and `SyncRecords`, under --mesh-purpose (default
//!       `data-integration`).
//!
//!       With --heimdall-addr, signed-in users the policy lets `Read` it get Heimdall's
//!       common operating picture at GET /api/v1/heimdall/picture. The gateway reads it
//!       from `heimdall-feed` over PQ-mTLS as the identity in --heimdall-identity-dir
//!       (service --heimdall-service, default `picture-analyst`; Heimdall's policy must
//!       grant it ReadPicture), declaring --heimdall-purpose (default
//!       `situational-awareness`).
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
  http-gateway --state-dir DIR [--bind ADDR] [--allow-origin URL]...
               [--mesh-bind ADDR --mesh-identity-dir DIR [--mesh-service NAME] [--mesh-purpose P]]
               [--heimdall-addr HOST:PORT --heimdall-identity-dir DIR [--heimdall-service NAME]
                [--heimdall-purpose P]]";

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
    let mut heimdall_purpose = "situational-awareness".to_string();
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
            "--heimdall-purpose" => heimdall_purpose = value.clone(),
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
    let (mut gateway, _lock) = served.into_parts();
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
    let gateway = Arc::new(gateway);

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

fn fail_usage(message: &str) -> ! {
    eprintln!("http-gateway: {message}\n{USAGE}");
    exit(2);
}
