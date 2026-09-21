//! `forge-egress` — render the egress broker's HAProxy configuration from the worker's
//! routes file, refusing it if any job would reach around the broker. [NFR-SEC-1]
//! [NFR-PRV-1]
//!
//! ```text
//! forge-egress --routes FILE [--jobs FILE] [--bind ADDR] [--ca-dir DIR] [--out FILE]
//! ```
//!
//! Prints the configuration on standard output, or writes it to `--out` (so a pod's init
//! container needs no shell, and runs through the image's verifying entrypoint). The
//! file is written only once every check has passed. `--bind` is the address the broker
//! listens on (default `127.0.0.1`, beside the worker in its pod); `--ca-dir` is where
//! the broker finds each route's CA file (default `/etc/egress/ca`). With `--jobs`, every
//! network source must be one of the routes' local ports. Exits 1 on a refused file.

use std::path::Path;

use forge_worker::egress::{check_jobs, parse_routes, render_haproxy};
use forge_worker::parse_jobs;
use service_runtime::Args;

const USAGE: &str =
    "usage: forge-egress --routes FILE [--jobs FILE] [--bind ADDR] [--ca-dir DIR] [--out FILE]";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(&argv, &["routes", "jobs", "bind", "ca-dir", "out"], &[]) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("forge-egress: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let written = run(&args).and_then(|config| match args.get("out") {
        Some(out) => std::fs::write(out, config).map_err(|e| format!("cannot write {out}: {e}")),
        None => {
            print!("{config}");
            Ok(())
        }
    });
    match written {
        Ok(()) => {}
        Err(e) => {
            eprintln!("forge-egress: {e}");
            std::process::exit(1);
        }
    }
}

fn run(args: &Args) -> Result<String, String> {
    let read =
        |path: &str| std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"));
    let routes_path = args.require("routes")?;
    let routes = parse_routes(&read(routes_path)?).map_err(|e| format!("{routes_path}: {e}"))?;
    if let Some(jobs_path) = args.get("jobs") {
        let base = Path::new(jobs_path).parent().unwrap_or(Path::new("."));
        let jobs = parse_jobs(&read(jobs_path)?, base).map_err(|e| format!("{jobs_path}: {e}"))?;
        check_jobs(&jobs, &routes)?;
    }
    Ok(render_haproxy(
        &routes,
        args.get("bind").unwrap_or("127.0.0.1"),
        args.get("ca-dir").unwrap_or("/etc/egress/ca"),
    ))
}
