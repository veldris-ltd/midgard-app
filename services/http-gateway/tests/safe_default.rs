//! The gateway is safe by default: with no arguments, or only an address, it refuses
//! to start; development mode needs the explicit `--dev` flag and listens on a loopback
//! address only. [MG-SEC-06] [NFR-SEC-1]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use http_gateway::cli::{loopback_addr, parse, Command as Cli};

const BIN: &str = env!("CARGO_BIN_EXE_http-gateway");

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Run the binary to completion. Every refusal exits before binding, so this never
/// blocks; a run that did start would hang the test, which is itself a failure.
fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run http-gateway")
}

fn refused(args: &[&str], why: &str) {
    let out = run(args);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "{args:?} must be refused as a usage error; stderr: {stderr}"
    );
    assert!(
        stderr.contains(why),
        "{args:?}: the refusal must say {why:?}; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("listening on"),
        "{args:?} must not listen: {stderr}"
    );
}

#[test]
fn no_arguments_refuses_to_start() {
    refused(&[], "a state directory is required");
}

#[test]
fn a_bare_address_no_longer_starts_development_mode() {
    refused(&["0.0.0.0:7710"], "no longer starts development mode");
    refused(&["127.0.0.1:7710"], "no longer starts development mode");
}

#[test]
fn options_without_a_state_directory_are_refused() {
    refused(&["--bind", "0.0.0.0:7710"], "--state-dir is required");
    refused(
        &["--allow-origin", "http://localhost:5173"],
        "--state-dir is required",
    );
}

#[test]
fn development_mode_refuses_a_non_loopback_address() {
    refused(&["--dev", "0.0.0.0:7710"], "loopback address only");
    refused(&["--dev", "[::]:7710"], "loopback address only");
    refused(&["--dev", "10.1.2.3:7710"], "loopback address only");
}

#[test]
fn development_mode_takes_no_other_option() {
    for args in [
        &["--dev", "127.0.0.1:0", "--state-dir", "x"][..],
        &["--state-dir", "x", "--dev"][..],
        &["--dev", "--bind", "0.0.0.0:7710"][..],
    ] {
        refused(args, "--dev takes at most one argument");
    }
}

#[test]
fn a_missing_state_directory_fails_closed() {
    let dir = std::env::temp_dir().join("mg-sec-06-no-such-state-dir");
    let _ = std::fs::remove_dir_all(&dir);
    let out = run(&[
        "--state-dir",
        dir.to_str().unwrap(),
        "--bind",
        "127.0.0.1:0",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("cannot serve"), "stderr: {stderr}");
    assert!(!stderr.contains("listening on"), "stderr: {stderr}");
}

#[test]
fn the_parser_says_what_each_command_line_means() {
    assert_eq!(
        parse(&argv(&["--dev"])),
        Ok(Cli::Dev("127.0.0.1:7710".parse().unwrap()))
    );
    assert_eq!(
        parse(&argv(&["--dev", "[::1]:8080"])),
        Ok(Cli::Dev("[::1]:8080".parse().unwrap()))
    );
    let Ok(Cli::Serve(serve)) = parse(&argv(&["--state-dir", "/state", "--bind", "0.0.0.0:7710"]))
    else {
        panic!("a state directory serves");
    };
    assert_eq!(serve.state_dir, "/state");
    assert_eq!(serve.bind, "0.0.0.0:7710");
    assert!(parse(&[]).is_err());
}

#[test]
fn only_loopback_addresses_pass_the_development_check() {
    assert!(loopback_addr("127.0.0.1:7710").is_ok());
    assert!(loopback_addr("[::1]:7710").is_ok());
    for open in ["0.0.0.0:7710", "[::]:7710", "192.168.1.10:7710"] {
        assert!(loopback_addr(open).is_err(), "{open} must be refused");
    }
}

#[test]
fn the_library_refuses_development_mode_off_loopback() {
    let err = http_gateway::serve("0.0.0.0:0").expect_err("refused before binding");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    let err = http_gateway::serve_dev("0.0.0.0:0".parse().unwrap()).expect_err("refused");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

/// `--dev` on loopback starts, says it is development mode, and listens only where it
/// was told.
#[cfg(feature = "backend-fips")]
#[test]
fn explicit_development_mode_serves_on_loopback() {
    let mut child = Command::new(BIN)
        .args(["--dev", "127.0.0.1:0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start http-gateway --dev");
    let mut stderr = BufReader::new(child.stderr.take().expect("stderr"));
    let mut seen = String::new();
    let addr = loop {
        let mut line = String::new();
        if stderr.read_line(&mut line).unwrap_or(0) == 0 {
            let _ = child.kill();
            panic!("the development gateway exited: {seen}");
        }
        seen.push_str(&line);
        if let Some(rest) = line
            .trim()
            .strip_prefix("http-gateway (secured) listening on http://")
        {
            break rest.to_string();
        }
    };
    assert!(seen.contains("DEVELOPMENT MODE"), "stderr: {seen}");
    assert!(
        addr.starts_with("127.0.0.1:"),
        "listens on loopback: {addr}"
    );

    let mut stream = TcpStream::connect(&addr).expect("connect to the dev edge");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
        .write_all(b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    let _ = child.kill();
    let _ = child.wait();
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "got: {resp}");
}
