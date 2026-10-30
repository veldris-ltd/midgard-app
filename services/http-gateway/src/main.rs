//! http-gateway binary — the browser-facing HTTP/JSON edge for the console UIs.
//!
//! Usage:
//!   http-gateway --state-dir DIR [--audit-pin FILE] [--checkpoint-mirror DIR] [--bind ADDR]
//!                [--allow-origin URL]... [--mesh-bind ADDR --mesh-identity-dir DIR [--mesh-service NAME]
//!                 [--mesh-purpose P]]
//!       Serve the enclosure in DIR, created by `mg-admin init`: persisted keys, the
//!       durable store, ledger and identity directory, and DIR/policy.json. Nothing
//!       is seeded and there is no built-in login. Each --allow-origin adds a browser
//!       origin to the CORS allowlist, which is otherwise empty.
//!
//!       --audit-pin names the audit verifying key the ledger is checked under — a
//!       copy of `trust/audit.pub` kept outside DIR (a read-only mount, a config
//!       map). Without it the gateway pins DIR/trust/audit.pub and says so: that
//!       still refuses a swapped audit keystore, but not one swapped together with
//!       the trust file beside it. [MG-SEC-05]
//!
//!       --checkpoint-mirror names a directory on another mount (NFS, an object-store
//!       FUSE mount, a separate volume) that every audit ledger checkpoint is also
//!       written to. The gateway refuses to boot a ledger that is behind it — rolled
//!       back together with its local checkpoint, or restored from an older backup —
//!       and an append whose checkpoint the mirror does not take fails. [MG-SEC-05]
//!
//!       With --mesh-bind, the gateway also serves `SyncRecords` over PQ-mTLS, so a
//!       certified workload — the Forge worker — can sync objects and links into the
//!       fabric. It presents the identity `mg-admin mesh issue --service NAME` wrote to
//!       --mesh-identity-dir (NAME defaults to `http-gateway`); policy.json decides
//!       who may open the channel and `SyncRecords`, under --mesh-purpose (default
//!       `data-integration`).
//!   http-gateway --dev [ADDR]
//!       Development mode (default 127.0.0.1:7710): an illustrative data set seeded
//!       into an in-memory fabric, the published demo logins, an ephemeral issuer key.
//!       It listens on a loopback ADDR only; any other address is refused.
//!
//! Started with no arguments, or with only an address, the gateway refuses to start:
//! development mode is never the default. [MG-SEC-06]
//!
//! Point a console at it with `VITE_GATEWAY=http` (see products/*/ui). [RC-24] [RC-29]
//! [FR-F-4] [NFR-SEC-1]

use std::net::SocketAddr;
use std::process::exit;
use std::sync::Arc;

use http_gateway::boot::{open_state_dir_mirrored, read_audit_pin, DurableGateway};
use http_gateway::cli::{self, Command, ServeArgs, USAGE};
use http_gateway::mesh_sync::spawn_mesh_listener;
use http_gateway::policy_file::parse_policy;
use http_gateway::state_dir::StateDir;
use policy::DefaultPdp;
use service_runtime::ServiceIdentity;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match cli::parse(&argv) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            return;
        }
        Ok(Command::Dev(addr)) => {
            eprintln!(
                "http-gateway: DEVELOPMENT MODE — seeded data and the published demo logins, \
                 on loopback only. Never use it for real data. [MG-SEC-06]"
            );
            if let Err(e) = http_gateway::serve_dev(addr) {
                eprintln!("http-gateway: failed to bind {addr}: {e}");
                exit(1);
            }
            return;
        }
        Ok(Command::Serve(args)) => args,
        Err(message) => fail_usage(&message),
    };
    let ServeArgs {
        state_dir,
        audit_pin,
        checkpoint_mirror,
        bind,
        origins,
        mesh_bind,
        mesh_identity_dir,
        mesh_service,
        mesh_purpose,
    } = args;

    let state = StateDir::new(&state_dir);
    let pin_path = match audit_pin {
        Some(path) => std::path::PathBuf::from(path),
        None => {
            let path = state.audit_trust_anchor();
            eprintln!(
                "http-gateway: no --audit-pin given; pinning the audit key in {}, inside the \
                 state directory. Pass --audit-pin with a copy kept outside it.",
                path.display()
            );
            path
        }
    };
    let pin = match read_audit_pin(&pin_path) {
        Ok(pin) => pin,
        Err(e) => {
            eprintln!(
                "http-gateway: cannot serve {state_dir}: cannot read the pinned audit key {}: {e}",
                pin_path.display()
            );
            exit(1);
        }
    };
    let mirror = checkpoint_mirror.as_deref().map(std::path::Path::new);
    let served = match open_state_dir_mirrored(&state, origins, &pin, mirror) {
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
