//! The edge's security regressions, driven over a real socket. Each test starts its own
//! development gateway on a fresh ephemeral port, so no test shares state with another,
//! and speaks raw HTTP to it so a request can be as malformed as an attacker's.
//!
//! This file is the shared harness for the edge security work packages: later ones add
//! their tests here, using [`spawn_dev_gateway`], [`raw_request`], [`post_json`] and
//! [`get`]. [VLD-ANB-MIDGARD-0006] [NFR-SEC-1] [AC-30.1] [AC-30.6]
#![cfg(feature = "backend-fips")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use http_gateway::http::MAX_BODY_BYTES;
use http_gateway::json::parse;
use http_gateway::{SecuredGateway, DEMO_PASSWORD, DEMO_USERNAME};

/// A response: status, headers in order, body.
pub type Reply = (u16, Vec<(String, String)>, String);

/// Start a development gateway on a fresh loopback port and return its address. The
/// listener is bound before this returns, so the first request cannot race the start.
pub fn spawn_dev_gateway() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("the bound address");
    let gw = Arc::new(SecuredGateway::dev());
    thread::spawn(move || {
        let _ = http_gateway::serve_listener(listener, gw);
    });
    addr
}

/// Send `bytes` on a fresh connection and read the response to EOF.
pub fn raw_request(addr: SocketAddr, bytes: &[u8]) -> Reply {
    let mut stream = TcpStream::connect(addr).expect("connect to the edge");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    // The edge may answer and stop reading before the request is all sent.
    let _ = stream.write_all(bytes);
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    parse_response(&String::from_utf8_lossy(&resp))
}

/// POST `body` to `path` as JSON.
pub fn post_json(addr: SocketAddr, path: &str, body: &str) -> Reply {
    post_json_with(addr, path, body, "")
}

/// GET `path`.
pub fn get(addr: SocketAddr, path: &str) -> Reply {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    raw_request(addr, req.as_bytes())
}

/// [`post_json`] with extra header lines, each ending `\r\n`.
fn post_json_with(addr: SocketAddr, path: &str, body: &str, extra: &str) -> Reply {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         {extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    raw_request(addr, req.as_bytes())
}

fn parse_response(text: &str) -> Reply {
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text, ""));
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status line in {text:?}"));
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    (status, headers, body.to_string())
}

/// Log in as the demo operator; the bearer token.
fn demo_token(addr: SocketAddr) -> String {
    let login = format!(r#"{{"username":"{DEMO_USERNAME}","password":"{DEMO_PASSWORD}"}}"#);
    let (status, _, body) = post_json(addr, "/api/v1/login", &login);
    assert_eq!(status, 200, "{body}");
    parse(&body)
        .ok()
        .and_then(|j| j.str_field("token").map(str::to_string))
        .expect("login returns a token")
}

fn assert_healthy(addr: SocketAddr) {
    let (status, _, body) = get(addr, "/api/v1/health");
    assert_eq!(status, 200, "the edge is still up: {body}");
}

// ---- WP-30: JSON nesting depth -------------------------------------------------

#[test]
fn a_deeply_nested_login_body_is_400_and_the_edge_stays_up() {
    // [VLD-ANB-MIDGARD-0006] [AC-30.1]
    let addr = spawn_dev_gateway();
    let start = Instant::now();
    let (status, _, body) = post_json(addr, "/api/v1/login", &"[".repeat(100_000));
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("malformed login body"), "{body}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "took {:?}",
        start.elapsed()
    );
    assert_healthy(addr);
}

#[test]
fn a_deeply_nested_authenticated_query_is_400() {
    // [VLD-ANB-MIDGARD-0006] [AC-30.1]
    let addr = spawn_dev_gateway();
    let token = demo_token(addr);
    let nested = "[".repeat(10_000) + &"]".repeat(10_000);
    let (status, _, body) = post_json_with(
        addr,
        "/api/v1/query",
        &nested,
        &format!("Authorization: Bearer {token}\r\n"),
    );
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("nesting deeper"), "{body}");
    assert_healthy(addr);
}

#[test]
fn a_body_of_one_mib_of_brackets_is_400() {
    // [VLD-ANB-MIDGARD-0006] [AC-30.1]
    let addr = spawn_dev_gateway();
    let body = "[".repeat(MAX_BODY_BYTES);
    let (status, _, reply) = post_json(addr, "/api/v1/login", &body);
    assert_eq!(status, 400, "{reply}");
    assert!(reply.contains("malformed login body"), "{reply}");
    assert_healthy(addr);
}
