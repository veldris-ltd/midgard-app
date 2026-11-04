//! End-to-end tests for the HTTP/JSON edge's unauthenticated surface: the pure
//! `route()` entry, which serves only the static platform read models, and a real
//! socket round-trip through `serve()`.
//!
//! The object fabric, governance and audit routes are caller-scoped and live behind
//! `SecuredGateway`; they are exercised end to end in `fabric_edge.rs`. [RC-37]
//! A request must arrive whole within the head deadline. [VLD-ANB-MIDGARD-0004]
//! [AC-31.2] `X-Forwarded-For` is read, bounded, and believed only from a trusted
//! proxy. [VLD-ANB-MIDGARD-0005]

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use http_gateway::data::State;
use http_gateway::http::{
    forwarded_client, read_request_within, Incoming, Request, MAX_FORWARDED_FOR_BYTES,
};
use http_gateway::route;

fn req(method: &str, path: &str, query: &str, body: &str) -> Request {
    Request {
        method: method.to_string(),
        path: path.to_string(),
        query: query.to_string(),
        body: body.to_string(),
        authorization: None,
        origin: None,
        forwarded_for: None,
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

/// Send `head` (and nothing more) to a fresh socket and read it back as the edge does.
fn read_head(head: &[u8]) -> Incoming {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    client.write_all(head).unwrap();
    read_request_within(&server, Duration::from_secs(5))
        .unwrap()
        .expect("a whole head")
}

#[test]
fn x_forwarded_for_is_read_joined_and_bounded() {
    // [VLD-ANB-MIDGARD-0005]
    let Incoming::Request(req) = read_head(
        b"POST /api/v1/login HTTP/1.1\r\nHost: edge\r\n\
          X-Forwarded-For: 203.0.113.9\r\nx-forwarded-for: 198.51.100.7\r\n\r\n",
    ) else {
        panic!("the request is served");
    };
    assert_eq!(
        req.forwarded_for.as_deref(),
        Some("203.0.113.9, 198.51.100.7"),
        "repeated lines are one list, in order"
    );
    let Incoming::Request(req) = read_head(b"GET /api/v1/health HTTP/1.1\r\n\r\n") else {
        panic!("the request is served");
    };
    assert_eq!(req.forwarded_for, None);

    let long = format!(
        "GET /api/v1/health HTTP/1.1\r\nX-Forwarded-For: {}\r\n\r\n",
        "1".repeat(MAX_FORWARDED_FOR_BYTES + 1)
    );
    let Incoming::Refused { response, .. } = read_head(long.as_bytes()) else {
        panic!("an over-long X-Forwarded-For is refused");
    };
    assert_eq!(response.status, 400);
}

#[test]
fn the_forwarded_client_is_the_rightmost_address_no_trusted_proxy_wrote() {
    // [VLD-ANB-MIDGARD-0005]
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    let proxies: BTreeSet<IpAddr> = [ip("10.0.0.1"), ip("10.0.0.2")].into();
    for (header, client) in [
        (Some("198.51.100.7"), Some("198.51.100.7")),
        // The left is the client's own claim; the proxy appended the right.
        (Some("203.0.113.1, 198.51.100.7"), Some("198.51.100.7")),
        // Trusted hops are walked past.
        (Some("198.51.100.7, 10.0.0.2"), Some("198.51.100.7")),
        (Some(" 198.51.100.7:4711 ,10.0.0.2"), Some("198.51.100.7")),
        (Some("[2001:db8::7]:443"), Some("2001:db8::7")),
        (Some("::ffff:198.51.100.7"), Some("198.51.100.7")),
        // An entry that is not an address stops the walk: nothing left of it is
        // believed.
        (Some("198.51.100.7, unknown"), None),
        (Some("10.0.0.1, 10.0.0.2"), None),
        (Some(""), None),
        (None, None),
    ] {
        assert_eq!(
            forwarded_client(header, &proxies),
            client.map(ip),
            "{header:?}"
        );
    }
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
