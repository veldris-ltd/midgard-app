//! `bifrost-agent` — the on-host fleet agent: checks an installation in with the fleet
//! manager over PQ-mTLS, or, as an operator, reads the fleet. [FR-B-3] [RC-17]
//! [NFR-SEC-1]
//!
//! ```text
//! bifrost-agent --identity-dir DIR --addr HOST:PORT --channel C --version V
//!               [--service NAME] [--install-id ID] [--unhealthy] [--purpose P]
//! bifrost-agent --identity-dir DIR --addr HOST:PORT --status [--service NAME] [--purpose P]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME defaults
//! to `fleet-agent`). An installation checks in under its own name: `--install-id`
//! defaults to NAME, and the manager refuses a check-in for any other. `--status` prints
//! the fleet as JSON. Exits 1 when the manager refuses.

use bifrost_fleet::AgentReport;
use bifrost_fleet_server::{outcome, CHECK_IN, FLEET_STATUS};
use bifrost_rollout::VersionId;
use service_runtime::{client, Args, ServiceIdentity};

const USAGE: &str = "usage: bifrost-agent --identity-dir DIR --addr HOST:PORT \
                     (--channel C --version V [--install-id ID] [--unhealthy] | --status) \
                     [--service NAME] [--purpose P]";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(
        &argv,
        &[
            "identity-dir",
            "addr",
            "service",
            "purpose",
            "channel",
            "version",
            "install-id",
        ],
        &["unhealthy", "status"],
    ) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("bifrost-agent: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    match run(&args) {
        Ok(report) => println!("{report}"),
        Err(e) => {
            eprintln!("bifrost-agent: {e}");
            std::process::exit(1);
        }
    }
}

fn run(args: &Args) -> Result<String, String> {
    let service = args.get("service").unwrap_or("fleet-agent");
    let identity = ServiceIdentity::load(args.require("identity-dir")?, service)?;
    let addr = args.require("addr")?;
    let purpose = args.get("purpose").unwrap_or("fleet-management");

    if args.flag("status") {
        let response = client::call(&identity, addr, purpose, FLEET_STATUS, &[])?;
        return answer(&response);
    }

    let install_id = args.get("install-id").unwrap_or(service);
    let channel = args.require("channel")?;
    let version = args.require("version")?;
    let report = AgentReport::new(
        install_id,
        channel,
        VersionId::new(version),
        !args.flag("unhealthy"),
    );
    let response = client::call(&identity, addr, purpose, CHECK_IN, &report.encode())?;
    let message = answer(&response)?;
    Ok(format!(
        "checked in {install_id} on {channel} at {version}: {message}"
    ))
}

/// The manager's message on success; its refusal as an error.
fn answer(response: &[u8]) -> Result<String, String> {
    let (code, body) = response
        .split_first()
        .ok_or_else(|| "the fleet manager sent an empty response".to_string())?;
    let message = String::from_utf8_lossy(body).into_owned();
    match *code {
        outcome::ACCEPTED => Ok(message),
        outcome::DENIED => Err(format!("denied: {message}")),
        outcome::MALFORMED => Err(format!("malformed: {message}")),
        _ => Err(format!("failed: {message}")),
    }
}
