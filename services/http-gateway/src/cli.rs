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

use std::net::{SocketAddr, ToSocketAddrs};

/// Where the gateway listens unless told otherwise, in either mode.
pub const DEFAULT_BIND: &str = "127.0.0.1:7710";

/// The usage text.
pub const USAGE: &str = "\
usage:
  http-gateway --state-dir DIR [--audit-pin FILE] [--bind ADDR] [--allow-origin URL]...
               [--mesh-bind ADDR --mesh-identity-dir DIR [--mesh-service NAME] [--mesh-purpose P]]
  http-gateway --dev [ADDR]
      development mode: seeded in-memory data and the published demo logins,
      on a loopback ADDR only (default 127.0.0.1:7710)";

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Print the usage and exit.
    Help,
    /// The development gateway, on this loopback address. [MG-SEC-06]
    Dev(SocketAddr),
    /// Serve the enclosure in a state directory.
    Serve(ServeArgs),
}

/// The options of [`Command::Serve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeArgs {
    /// The state directory `mg-admin init` created.
    pub state_dir: String,
    /// The audit verifying key the ledger is checked under, kept outside the state
    /// directory. `None` pins the copy inside it. [MG-SEC-05]
    pub audit_pin: Option<String>,
    /// The HTTP bind address.
    pub bind: String,
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
}

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
        audit_pin: None,
        bind: DEFAULT_BIND.to_string(),
        origins: Vec::new(),
        mesh_bind: None,
        mesh_identity_dir: None,
        mesh_service: "http-gateway".to_string(),
        mesh_purpose: "data-integration".to_string(),
    };
    let mut args = argv.iter();
    while let Some(flag) = args.next() {
        let Some(value) = args.next().filter(|v| !v.starts_with("--")) else {
            return Err(format!("{flag} requires a value"));
        };
        match flag.as_str() {
            "--state-dir" if state_dir.is_none() => state_dir = Some(value.clone()),
            "--state-dir" => return Err("--state-dir given twice".into()),
            "--audit-pin" if serve.audit_pin.is_none() => serve.audit_pin = Some(value.clone()),
            "--audit-pin" => return Err("--audit-pin given twice".into()),
            "--bind" => serve.bind = value.clone(),
            "--allow-origin" => serve.origins.push(value.clone()),
            "--mesh-bind" => serve.mesh_bind = Some(value.clone()),
            "--mesh-identity-dir" => serve.mesh_identity_dir = Some(value.clone()),
            "--mesh-service" => serve.mesh_service = value.clone(),
            "--mesh-purpose" => serve.mesh_purpose = value.clone(),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let Some(state_dir) = state_dir.filter(|d| !d.is_empty()) else {
        return Err("--state-dir is required".into());
    };
    if serve.mesh_bind.is_some() != serve.mesh_identity_dir.is_some() {
        return Err("--mesh-bind and --mesh-identity-dir go together".into());
    }
    serve.state_dir = state_dir;
    Ok(Command::Serve(serve))
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
            "development mode listens on a loopback address only, and {addr} is {}; serve a \
             state directory (--state-dir) to listen on another interface",
            open.ip()
        ));
    }
    Ok(first)
}
