//! `bifrost-agent` — the on-host fleet agent: checks an installation in with the fleet
//! manager over PQ-mTLS, or, as an operator, reads the fleet; or, in suite mode, keeps
//! this Mac's native suite at the versions the orchestrator desires. [FR-B-2] [FR-B-3]
//! [RC-17] [NFR-SEC-1]
//!
//! ```text
//! bifrost-agent --identity-dir DIR --addr HOST:PORT --channel C --version V
//!               [--service NAME] [--install-id ID] [--unhealthy] [--purpose P]
//! bifrost-agent --identity-dir DIR --addr HOST:PORT --status [--service NAME] [--purpose P]
//! bifrost-agent --suite --identity-dir DIR --addr HOST:PORT --service NAME
//!               --suite-root DIR --runner CMD [--install-id ID] [--channel C]
//!               [--initial-version V] [--adopt-from DIR] [--trust-dir DIR]
//!               [--services a,b,…] [--interval SECS] [--purpose P]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME defaults
//! to `fleet-agent`). An installation checks in under its own name: `--install-id`
//! defaults to NAME, and the manager refuses a check-in for any other. `--status` prints
//! the fleet as JSON. Exits 1 when the manager refuses.
//!
//! `--suite` runs until it is stopped (see `bifrost_fleet_server::suite`): every
//! `--interval` (10 s) it checks in each service's version and readiness with
//! `CheckInServices` on `--channel` (stable), installs the desired builds into
//! `--suite-root`'s slots after verifying them against `--trust-dir`'s release signers
//! (default `<suite-root>/trust`), and restarts them with `--runner restart <service>`.
//! On the first run it adopts the binaries in `--adopt-from` (default `dist/mac/bin`) as
//! `--initial-version`, if they match the registry. `--services` defaults to the six suite
//! services. It exits 1 only on a refusal an operator must resolve.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bifrost_fleet::AgentReport;
use bifrost_fleet_server::registry::load_signers;
use bifrost_fleet_server::suite::{self, AgentConfig, CommandRunner, Slots, SUITE_SERVICES};
use bifrost_fleet_server::{outcome, CHECK_IN, FLEET_STATUS};
use bifrost_rollout::VersionId;
use service_runtime::{client, Args, ServiceIdentity};

const USAGE: &str = "usage: bifrost-agent --identity-dir DIR --addr HOST:PORT \
                     (--channel C --version V [--install-id ID] [--unhealthy] | --status) \
                     [--service NAME] [--purpose P]\n       \
                     bifrost-agent --suite --identity-dir DIR --addr HOST:PORT --service NAME \
                     --suite-root DIR --runner CMD [--install-id ID] [--channel C] \
                     [--initial-version V] [--adopt-from DIR] [--trust-dir DIR] \
                     [--services a,b,...] [--interval SECS] [--purpose P]";

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
            "suite-root",
            "runner",
            "initial-version",
            "adopt-from",
            "trust-dir",
            "services",
            "interval",
        ],
        &["unhealthy", "status", "suite"],
    ) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("bifrost-agent: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let result = if args.flag("suite") {
        run_suite(&args).map(|()| String::new())
    } else {
        run(&args)
    };
    match result {
        Ok(report) => println!("{report}"),
        Err(e) => {
            if args.flag("suite") {
                suite::log("error", &e);
            }
            eprintln!("bifrost-agent: {e}");
            std::process::exit(1);
        }
    }
}

fn run_suite(args: &Args) -> Result<(), String> {
    for only in ["version", "unhealthy", "status"] {
        if args.get(only).is_some() || args.flag(only) {
            return Err(format!("--{only} is not a --suite option"));
        }
    }
    let service = args.require("service")?;
    let identity = ServiceIdentity::load(args.require("identity-dir")?, service)?;
    let root = PathBuf::from(args.require("suite-root")?);
    let slots = Slots::open(&root).map_err(|e| format!("cannot open {}: {e}", root.display()))?;
    let trust = args
        .get("trust-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("trust"));
    let anchors = load_signers(&trust)?;
    let services: Vec<String> = match args.get("services") {
        Some(list) => list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        None => SUITE_SERVICES.iter().map(|s| s.to_string()).collect(),
    };
    let interval = match args.get("interval") {
        Some(s) => s
            .parse::<u64>()
            .ok()
            .filter(|n| (1..=3600).contains(n))
            .ok_or("--interval must be 1 to 3600 seconds")?,
        None => 10,
    };
    suite::run(AgentConfig {
        identity: Arc::new(identity),
        addr: args.require("addr")?.to_string(),
        purpose: args
            .get("purpose")
            .unwrap_or("fleet-management")
            .to_string(),
        install_id: args.get("install-id").unwrap_or(service).to_string(),
        channel: args.get("channel").unwrap_or("stable").to_string(),
        slots,
        runner: Arc::new(CommandRunner::new(args.require("runner")?)?),
        anchors,
        services,
        initial_version: args.get("initial-version").map(str::to_string),
        adopt_from: PathBuf::from(args.get("adopt-from").unwrap_or("dist/mac/bin")),
        interval: Duration::from_secs(interval),
    })
}

fn run(args: &Args) -> Result<String, String> {
    for only in [
        "suite-root",
        "runner",
        "initial-version",
        "adopt-from",
        "trust-dir",
        "services",
        "interval",
    ] {
        if args.get(only).is_some() {
            return Err(format!("--{only} needs --suite"));
        }
    }
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
