//! The gateway's command line, parsed apart from `main` so what it refuses can be
//! tested. [RC-29] [MG-SEC-06]
//!
//! The gateway is safe by default: started with nothing, it serves nothing. It needs
//! a state directory (`--state-dir`, created by `mg-admin init`), or the explicit
//! `--dev` flag for the seeded, in-memory development gateway with its published demo
//! logins — and development mode listens on a loopback address only, whatever address
//! it is given. A bare address no longer starts development mode, so an image or a
//! script that passes only `0.0.0.0:7710` is refused rather than serving the demo
//! logins on every interface. [MG-SEC-06] [NFR-SEC-1]
//!
//! What each listener serves within can be tuned: `--max-connections` and
//! `--idle-timeout` bound the edge and the mesh door alike, `--edge-rate-per-sec` is how
//! fast one source address may open connections to the edge (0 turns that off), and
//! `--rate-burst`, `--rate-per-sec` and `--requests-per-conn` are the mesh door's
//! per-identity rate limit and request quota. [VLD-ANB-MIDGARD-0004]
//!
//! `--login-burst` and `--login-per-sec` are how many login and administration attempts
//! one source address may make at once, and how fast that allowance refills (a burst
//! of 0 turns it off). [VLD-ANB-MIDGARD-0005]
//!
//! The edge speaks plaintext HTTP, so a state-directory gateway also listens on
//! loopback only unless `--allow-non-loopback` says it sits behind PQ-mTLS or TLS 1.3
//! termination, as a container does. Development mode has no such override.
//! [RC-24] [VLD-ANB-MIDGARD-0032]

use std::net::{SocketAddr, ToSocketAddrs};

use policy::{Clearance, Compartment};
use service_runtime::marking::{level_of, BANDS};
use service_runtime::{mesh_limits_from_args, Args, MeshLimits};

use crate::login_throttle::{LOGIN_SOURCE_BURST, LOGIN_SOURCE_PER_SEC};
use crate::{check_bind, BindPolicy, EdgeLimits};

/// Where the gateway listens unless told otherwise, in either mode.
pub const DEFAULT_BIND: &str = "127.0.0.1:7710";

/// The usage text.
pub const USAGE: &str = "\
usage:
  http-gateway --state-dir DIR [--keystore-passphrase-file FILE] [--audit-pin FILE]
               [--checkpoint-mirror DIR] [--bind ADDR]
               [--allow-non-loopback] [--allow-origin URL]...
               [--mesh-bind ADDR --mesh-identity-dir DIR [--mesh-service NAME] [--mesh-purpose P]
                [--mesh-sync-clearance BAND[:COMPARTMENT,...]]]
               [--max-connections N] [--idle-timeout SECONDS] [--edge-rate-per-sec N]
               [--rate-burst N] [--rate-per-sec N] [--requests-per-conn N]
               [--login-burst N] [--login-per-sec N]
      --mesh-sync-clearance: the highest marking a mesh sync may write (default: the
      highest band, no compartments)
      --max-connections (edge default 256, mesh door 128) and --idle-timeout (default 30)
      bound each listener; --edge-rate-per-sec (default 50, 0 is off) is how fast one
      address may open connections to the edge; --rate-burst (default 100),
      --rate-per-sec (default 20) and --requests-per-conn (default 10000) limit each
      certified peer of the mesh door; --login-burst (default 20, 0 is off) and
      --login-per-sec (default 0.5) limit login and administration attempts from one
      address
  http-gateway --dev [ADDR]
      development mode: seeded in-memory data and the published demo logins,
      on a loopback ADDR only (default 127.0.0.1:7710)
  --allow-non-loopback lets a state-directory gateway bind an address that is not
      loopback; the plaintext edge must then sit behind PQ-mTLS or TLS 1.3 termination.
      Development mode has no override.

  The keystores' custody roots are wrapped under a passphrase read from
  --keystore-passphrase-file, or the file MIDGARD_KEYSTORE_PASSPHRASE_FILE names;
  with neither the gateway refuses to start.";

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Print the usage and exit.
    Help,
    /// The development gateway, on this loopback address. [MG-SEC-06]
    Dev(SocketAddr),
    /// Serve the enclosure in a state directory.
    Serve(Box<ServeArgs>),
}

/// The options of [`Command::Serve`].
#[derive(Debug, Clone, PartialEq)]
pub struct ServeArgs {
    /// The state directory `mg-admin init` created.
    pub state_dir: String,
    /// The file holding the keystores' passphrase. `None` falls back to
    /// `MIDGARD_KEYSTORE_PASSPHRASE_FILE`, read by `main`. [VLD-ANB-MIDGARD-0007]
    pub keystore_passphrase_file: Option<String>,
    /// The audit verifying key the ledger is checked under, kept outside the state
    /// directory. `None` pins the copy inside it. [MG-SEC-05]
    pub audit_pin: Option<String>,
    /// A directory on another mount that every audit ledger checkpoint is also written
    /// to, and that the ledger must not be behind at boot. `None` keeps the checkpoint
    /// beside the ledger only. [MG-SEC-05]
    pub checkpoint_mirror: Option<String>,
    /// The HTTP bind address.
    pub bind: String,
    /// Whether `bind` may be an address that is not loopback (`--allow-non-loopback`).
    /// [RC-24] [VLD-ANB-MIDGARD-0032]
    pub bind_policy: BindPolicy,
    /// The browser origins the CORS allowlist admits.
    pub origins: Vec<String>,
    /// The PQ-mTLS mesh door's bind address, if it is served.
    pub mesh_bind: Option<String>,
    /// Where the mesh identity is, if the mesh door is served.
    pub mesh_identity_dir: Option<String>,
    /// The mesh identity's service name.
    pub mesh_service: String,
    /// The purpose mesh calls are made under.
    pub mesh_purpose: String,
    /// What the HTTP edge serves within. [VLD-ANB-MIDGARD-0004]
    pub edge: EdgeLimits,
    /// What the mesh door serves within. [VLD-ANB-MIDGARD-0004]
    pub mesh: MeshLimits,
    /// Login and administration attempts one source address may make at once; 0 turns
    /// the per-source limit off. [VLD-ANB-MIDGARD-0005]
    pub login_burst: f64,
    /// Attempts a second that allowance refills at. [VLD-ANB-MIDGARD-0005]
    pub login_per_sec: f64,
    /// The highest marking a mesh sync may write. `None` keeps the gateway's default:
    /// the highest band, no compartments. [VLD-ANB-MIDGARD-0001]
    pub mesh_sync_clearance: Option<Clearance>,
}

/// The options that tune the listeners' limits, given to [`limits`].
const LIMIT_OPTIONS: [&str; 6] = [
    "max-connections",
    "idle-timeout",
    "rate-burst",
    "rate-per-sec",
    "requests-per-conn",
    "edge-rate-per-sec",
];

/// Parse the arguments after the program name. An `Err` is a usage error: the caller
/// prints it with [`USAGE`] and exits non-zero without serving anything.
pub fn parse(argv: &[String]) -> Result<Command, String> {
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(Command::Help);
    }
    let Some(first) = argv.first() else {
        // Nothing given is not development mode any more: fail closed. [MG-SEC-06]
        return Err(
            "a state directory is required (--state-dir DIR); for the local development \
             gateway, pass --dev explicitly"
                .into(),
        );
    };
    if argv.iter().any(|a| a == "--dev") {
        if argv.iter().any(|a| a == "--allow-non-loopback") {
            return Err(
                "development mode listens on loopback only and has no --allow-non-loopback \
                 override; to listen on another interface, serve a state directory \
                 (--state-dir) behind PQ-mTLS or TLS 1.3 termination"
                    .into(),
            );
        }
        if first != "--dev" || argv.len() > 2 || argv.get(1).is_some_and(|a| a.starts_with("--")) {
            return Err(
                "--dev takes at most one argument, a loopback address, and no other option".into(),
            );
        }
        let addr = argv.get(1).map_or(DEFAULT_BIND, String::as_str);
        return loopback_addr(addr).map(Command::Dev);
    }
    if !first.starts_with("--") {
        return Err(format!(
            "a bare address ({first}) no longer starts development mode; serve a state \
             directory with --state-dir DIR --bind {first}, or run the development gateway \
             on loopback with --dev"
        ));
    }

    let mut state_dir = None;
    let mut serve = ServeArgs {
        state_dir: String::new(),
        keystore_passphrase_file: None,
        audit_pin: None,
        checkpoint_mirror: None,
        bind: DEFAULT_BIND.to_string(),
        bind_policy: BindPolicy::LoopbackOnly,
        origins: Vec::new(),
        mesh_bind: None,
        mesh_identity_dir: None,
        mesh_service: "http-gateway".to_string(),
        mesh_purpose: "data-integration".to_string(),
        edge: EdgeLimits::default(),
        mesh: MeshLimits::default(),
        login_burst: LOGIN_SOURCE_BURST,
        login_per_sec: LOGIN_SOURCE_PER_SEC,
        mesh_sync_clearance: None,
    };
    let mut limit_flags = Vec::new();
    let mut args = argv.iter();
    while let Some(flag) = args.next() {
        if flag == "--allow-non-loopback" {
            serve.bind_policy = BindPolicy::AllowNonLoopback;
            continue;
        }
        let Some(value) = args.next().filter(|v| !v.starts_with("--")) else {
            return Err(format!("{flag} requires a value"));
        };
        match flag.as_str() {
            "--state-dir" if state_dir.is_none() => state_dir = Some(value.clone()),
            "--state-dir" => return Err("--state-dir given twice".into()),
            "--keystore-passphrase-file" if serve.keystore_passphrase_file.is_none() => {
                serve.keystore_passphrase_file = Some(value.clone())
            }
            "--keystore-passphrase-file" => {
                return Err("--keystore-passphrase-file given twice".into())
            }
            "--audit-pin" if serve.audit_pin.is_none() => serve.audit_pin = Some(value.clone()),
            "--audit-pin" => return Err("--audit-pin given twice".into()),
            "--checkpoint-mirror" if serve.checkpoint_mirror.is_none() => {
                serve.checkpoint_mirror = Some(value.clone())
            }
            "--checkpoint-mirror" => return Err("--checkpoint-mirror given twice".into()),
            "--bind" => serve.bind = value.clone(),
            "--allow-origin" => serve.origins.push(value.clone()),
            "--mesh-bind" => serve.mesh_bind = Some(value.clone()),
            "--mesh-identity-dir" => serve.mesh_identity_dir = Some(value.clone()),
            "--mesh-service" => serve.mesh_service = value.clone(),
            "--mesh-purpose" => serve.mesh_purpose = value.clone(),
            "--login-burst" => serve.login_burst = non_negative(flag, value)?,
            "--login-per-sec" => serve.login_per_sec = non_negative(flag, value)?,
            other if LIMIT_OPTIONS.contains(&other.trim_start_matches("--")) => {
                limit_flags.extend([flag.clone(), value.clone()]);
            }
            "--mesh-sync-clearance" => {
                serve.mesh_sync_clearance = Some(parse_sync_clearance(value)?)
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    let Some(state_dir) = state_dir.filter(|d| !d.is_empty()) else {
        return Err("--state-dir is required".into());
    };
    if serve.mesh_bind.is_some() != serve.mesh_identity_dir.is_some() {
        return Err("--mesh-bind and --mesh-identity-dir go together".into());
    }
    // Refused here, before the state directory is opened or locked, as a usage error.
    check_bind(&serve.bind, serve.bind_policy)?;
    serve.state_dir = state_dir;
    (serve.edge, serve.mesh) = limits(&limit_flags)?;
    Ok(Command::Serve(Box::new(serve)))
}

/// The edge's and the mesh door's limits, from the limit options given (`--key value`
/// pairs): the defaults, with `--max-connections` and `--idle-timeout` applied to both.
/// [VLD-ANB-MIDGARD-0004]
fn limits(flags: &[String]) -> Result<(EdgeLimits, MeshLimits), String> {
    let args = Args::parse(flags, &LIMIT_OPTIONS, &[])?;
    let mesh = mesh_limits_from_args(&args)?;
    let mut edge = EdgeLimits::default();
    if args.get("max-connections").is_some() {
        edge.listener.max_connections = mesh.listener.max_connections;
    }
    if args.get("idle-timeout").is_some() {
        edge.listener.read_timeout = mesh.listener.read_timeout;
        edge.listener.write_timeout = mesh.listener.write_timeout;
    }
    if let Some(v) = args.get("edge-rate-per-sec") {
        match v.parse::<f64>() {
            Ok(n) if n == 0.0 => (edge.source_burst, edge.source_per_sec) = (0.0, 0.0),
            Ok(n) if n.is_finite() && n > 0.0 => edge.source_per_sec = n,
            _ => {
                return Err(format!(
                    "--edge-rate-per-sec must be a number of at least 0, not '{v}'"
                ))
            }
        }
    }
    Ok((edge, mesh))
}

/// `value` as a number of at least 0, or the usage error for `flag`.
fn non_negative(flag: &str, value: &str) -> Result<f64, String> {
    match value.parse::<f64>() {
        Ok(n) if n.is_finite() && n >= 0.0 => Ok(n),
        _ => Err(format!(
            "{flag} must be a number of at least 0, not '{value}'"
        )),
    }
}

/// Parse a sync clearance, `BAND[:COMPARTMENT,...]`: a marking band (`open`,
/// `internal`, `restricted`, `confidential`) and the compartments a mesh sync may write.
/// [VLD-ANB-MIDGARD-0001]
pub fn parse_sync_clearance(value: &str) -> Result<Clearance, String> {
    let (band, compartments) = value.split_once(':').unwrap_or((value, ""));
    let level = level_of(band).ok_or_else(|| {
        format!(
            "--mesh-sync-clearance: {band} is not a marking band (one of {})",
            BANDS.join(", ")
        )
    })?;
    let compartments = compartments
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| Compartment(c.to_string()))
        .collect();
    Ok(Clearance {
        level,
        compartments,
    })
}

/// Resolve `addr` for development mode, which listens on loopback only: every address
/// it resolves to must be a loopback address, or it is refused. The address returned is
/// the one to bind, so a name cannot resolve differently between this check and the
/// bind. [MG-SEC-06]
pub fn loopback_addr(addr: &str) -> Result<SocketAddr, String> {
    let resolved: Vec<SocketAddr> = addr
        .to_socket_addrs()
        .map_err(|e| format!("development mode cannot resolve {addr}: {e}"))?
        .collect();
    let first = *resolved
        .first()
        .ok_or_else(|| format!("development mode cannot resolve {addr}"))?;
    if let Some(open) = resolved.iter().find(|a| !a.ip().is_loopback()) {
        return Err(format!(
            "development mode listens on a loopback address only, and {addr} is {}; it has \
             no --allow-non-loopback override. To listen on another interface, serve a \
             state directory (--state-dir) behind PQ-mTLS or TLS 1.3 termination",
            open.ip()
        ));
    }
    Ok(first)
}
