//! End-to-end tests for the HTTP/JSON edge's unauthenticated surface: the pure
//! `route()` entry, which serves only the static platform read models, and a real
//! socket round-trip through `serve()`.
//!
//! The object fabric, governance and audit routes are caller-scoped and live behind
//! `SecuredGateway`; they are exercised end to end in `fabric_edge.rs`. [RC-37]
//! A request must arrive whole within the head deadline. [VLD-ANB-MIDGARD-0004]
//! [AC-31.2]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use http_gateway::data::State;
use http_gateway::http::{read_request_within, Request};
use http_gateway::route;

fn req(method: &str, path: &str, query: &str, body: &str) -> Request {
    Request {
        method: method.to_string(),
        path: path.to_string(),
        query: query.to_string(),
        body: body.to_string(),
        authorization: None,
        origin: None,
    }
}

#[test]
fn fabric_and_governance_routes_are_not_reachable_without_a_caller() {
    // Fabric and governance reads are marking-filtered and audited against an
    // authenticated identity, so the unauthenticated router must not serve them.
    let s = State::new();
    for (method, path) in [
        ("GET", "/api/v1/object-types"),
        ("POST", "/api/v1/query"),
        ("POST", "/api/v1/actions"),
        ("POST", "/api/v1/search-around"),
        ("GET", "/api/v1/audit"),
        ("GET", "/api/v1/policies"),
        ("POST", "/api/v1/policy/simulate"),
    ] {
        assert_eq!(
            route(&req(method, path, "", r#"{"objectType":"aircraft"}"#), &s).status,
            404,
            "{method} {path} must not be served without a caller"
        );
    }
}

#[test]
fn platform_reads_serve_json() {
    let s = State::new();
    for path in [
        "/api/v1/identities",
        "/api/v1/keys",
        "/api/v1/compute",
        "/api/v1/observability",
        "/api/v1/resilience",
        "/api/v1/sdk-surface",
    ] {
        let r = route(&req("GET", path, "", ""), &s);
        assert_eq!(r.status, 200, "{path} should be 200");
        assert!(
            r.body.starts_with('[') || r.body.starts_with('{'),
            "{path} should be JSON"
        );
    }
}

#[test]
fn cors_preflight_and_not_found() {
    let s = State::new();
    assert_eq!(
        route(&req("OPTIONS", "/api/v1/query", "", ""), &s).status,
        204
    );
    assert_eq!(route(&req("GET", "/api/v1/nope", "", ""), &s).status, 404);
}

#[test]
fn real_socket_round_trip() {
    let addr = "127.0.0.1:17711";
    thread::spawn(move || {
        let _ = http_gateway::serve(addr);
    });
    // give the listener a moment to bind
    thread::sleep(Duration::from_millis(300));

    // Public health from the allowlisted console origin: served, origin-locked.
    let mut stream = TcpStream::connect(addr).expect("connect to edge");
    stream
        .write_all(
            b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\n\
              Origin: http://localhost:5173\r\nConnection: close\r\n\r\n",
        )
        .unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "got: {resp}");
    // RT-24: origin-locked CORS — the allowlisted origin is echoed, never `*`.
    assert!(
        !resp.contains("Access-Control-Allow-Origin: *"),
        "no wildcard CORS"
    );
    assert!(resp.contains("Access-Control-Allow-Origin: http://localhost:5173"));
    assert!(resp.contains("\"status\":\"ok\""));

    // A foreign origin gets NO CORS grant.
    let mut stream = TcpStream::connect(addr).expect("connect to edge");
    stream
        .write_all(
            b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\n\
              Origin: https://evil.example\r\nConnection: close\r\n\r\n",
        )
        .unwrap();
    let mut resp2 = String::new();
    stream.read_to_string(&mut resp2).unwrap();
    assert!(resp2.starts_with("HTTP/1.1 200 OK"));
    assert!(
        !resp2.contains("Access-Control-Allow-Origin"),
        "no grant for foreign origin"
    );
}

#[test]
fn read_request_within_returns_none_when_the_head_deadline_passes() {
    // [VLD-ANB-MIDGARD-0004] [AC-31.2]
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    // Half a head, and then nothing.
    client
        .write_all(b"GET /api/v1/health HTTP/1.1\r\nHost: loc")
        .unwrap();

    let started = Instant::now();
    let read = read_request_within(&server, Duration::from_millis(500));
    let waited = started.elapsed();
    assert!(
        matches!(read, Ok(None)),
        "nothing is served from half a head"
    );
    assert!(
        waited >= Duration::from_millis(400) && waited < Duration::from_secs(2),
        "gave up after {waited:?}"
    );
}
