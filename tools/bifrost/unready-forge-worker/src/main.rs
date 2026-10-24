//! The "bad build" of forge-worker: it takes forge-worker's command line, starts, stays
//! alive and never becomes ready. `tools/bifrost/release.sh --bad-build` registers it as
//! forge-worker, the canary, so Bifrost's rollout gate (not ready within the timeout)
//! fails and the release rolls back. Release tooling only; std only.
//!
//! - Every argument is accepted and ignored, except `--health-bind ADDR`. On that,
//!   `/healthz` answers 200 (the process is alive) and `/readyz` 503 (it never becomes
//!   ready), as forge-worker's own health port would before its first job.
//! - It never logs `"message":"job run"`, the line run-native.sh waits for.
//! - It runs until it is signalled, like the service.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn log(level: &str, message: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!(
        "{{\"ts\":{ts},\"level\":\"{level}\",\"service\":\"forge-worker\",\
         \"build\":\"unready-stub\",\"message\":\"{message}\"}}"
    );
}

fn answer(stream: TcpStream) {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/");
    let (status, body) = match path {
        "/healthz" => ("200 OK", "ok\n"),
        "/readyz" => ("503 Service Unavailable", "not ready: bad build\n"),
        "/metrics" => ("200 OK", "forge_worker_ready 0\n"),
        _ => ("404 Not Found", "not found\n"),
    };
    let _ = write!(
        &stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let health = args
        .iter()
        .position(|a| a == "--health-bind")
        .and_then(|i| args.get(i + 1))
        .cloned();
    log("warn", "bad build: this forge-worker never becomes ready");
    if let Some(addr) = health {
        match TcpListener::bind(&addr) {
            Ok(listener) => {
                std::thread::spawn(move || {
                    for stream in listener.incoming().flatten() {
                        std::thread::spawn(move || answer(stream));
                    }
                });
            }
            Err(e) => {
                log("error", &format!("cannot bind the health port: {e}"));
                std::process::exit(1);
            }
        }
    }
    loop {
        std::thread::sleep(Duration::from_secs(30));
        log("warn", "not ready");
    }
}
