//! The edge's security regressions, driven over a real socket. Each test starts its own
//! development gateway on a fresh ephemeral port, so no test shares state with another,
//! and speaks raw HTTP to it so a request can be as malformed as an attacker's.
//!
//! This file is the shared harness for the edge security work packages: later ones add
//! their tests here, using [`spawn_dev_gateway`], [`raw_request`], [`post_json`] and
//! [`get`]; [`spawn_dev_gateway_with`] starts one within chosen limits.
//! [VLD-ANB-MIDGARD-0006] [VLD-ANB-MIDGARD-0004] [NFR-SEC-1] [AC-30.1] [AC-30.6]
//! [AC-31.1] [AC-31.2]
#![cfg(feature = "backend-fips")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use http_gateway::http::MAX_BODY_BYTES;
use http_gateway::json::parse;
use http_gateway::{EdgeLimits, ListenerLimits, SecuredGateway, DEMO_PASSWORD, DEMO_USERNAME};

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

/// [`spawn_dev_gateway`] serving within `limits`.
pub fn spawn_dev_gateway_with(limits: EdgeLimits) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("the bound address");
    let gw = Arc::new(SecuredGateway::dev());
    thread::spawn(move || {
        let _ = http_gateway::serve_listener_with(listener, gw, limits);
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

// ---- WP-31: bounded listener -----------------------------------------------------

/// The edge's limits with the listener's replaced by `listener`.
fn with_listener(listener: ListenerLimits) -> EdgeLimits {
    EdgeLimits {
        listener,
        ..EdgeLimits::default()
    }
}

/// Open `n` connections that send nothing.
fn idle_connections(addr: SocketAddr, n: usize) -> Vec<TcpStream> {
    (0..n)
        .map(|_| TcpStream::connect(addr).expect("connect to the edge"))
        .collect()
}

/// The value of header `name`, if the reply carries it.
fn header<'a>(reply: &'a Reply, name: &str) -> Option<&'a str> {
    reply
        .1
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Health answers 200 within `within` — slots free as the edge sees connections close.
fn healthy_within(addr: SocketAddr, within: Duration) {
    let deadline = Instant::now() + within;
    loop {
        let (status, _, body) = get(addr, "/api/v1/health");
        if status == 200 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "health is {status} after {within:?}: {body}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// Whether the edge closes `stream` (end of stream or a reset) within `within`, and
/// what it sent before it did.
fn closed_by_edge(stream: &mut TcpStream, within: Duration) -> (bool, String) {
    stream
        .set_read_timeout(Some(within + Duration::from_secs(2)))
        .unwrap();
    let started = Instant::now();
    let mut got = Vec::new();
    let closed = match stream.read_to_end(&mut got) {
        Ok(_) => true,
        Err(e) => !matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
    };
    (
        closed && started.elapsed() < within,
        String::from_utf8_lossy(&got).into_owned(),
    )
}

#[test]
fn idle_connections_past_the_cap_get_503_and_the_edge_still_answers() {
    // [VLD-ANB-MIDGARD-0004] [AC-31.1] [AC-31.2]
    let addr = spawn_dev_gateway_with(with_listener(ListenerLimits {
        max_connections: 16,
        ..ListenerLimits::edge()
    }));
    let mut idle = idle_connections(addr, 16);

    let started = Instant::now();
    let reply = get(addr, "/api/v1/health");
    assert_eq!(
        reply.0, 503,
        "the 17th connection is over the cap: {}",
        reply.2
    );
    assert_eq!(header(&reply, "Retry-After"), Some("1"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "refused at once, took {:?}",
        started.elapsed()
    );

    drop(idle.pop());
    healthy_within(addr, Duration::from_secs(2));
}

#[test]
fn three_hundred_idle_connections_do_not_exhaust_the_default_edge() {
    // [VLD-ANB-MIDGARD-0004] [AC-31.1]
    // The default listener limits. Every connection here comes from 127.0.0.1, so the
    // per-source allowance (its own test below) would answer 429 before the cap is
    // reached; it is off here so the cap is what is measured.
    let addr = spawn_dev_gateway_with(EdgeLimits {
        listener: ListenerLimits::edge(),
        source_burst: 0.0,
        source_per_sec: 0.0,
    });
    let started = Instant::now();
    let idle = idle_connections(addr, 300);

    let (mut refused, mut served) = (0, 0);
    for _ in 0..300 {
        let reply = get(addr, "/api/v1/health");
        match reply.0 {
            503 => {
                assert_eq!(header(&reply, "Retry-After"), Some("1"));
                refused += 1;
            }
            200 => served += 1,
            other => panic!("health answered {other}: {}", reply.2),
        }
    }
    assert!(refused >= 1, "{refused} refused, {served} served");

    drop(idle);
    healthy_within(addr, Duration::from_secs(5));
    for i in 0..20 {
        let (status, _, body) = get(addr, "/api/v1/health");
        assert_eq!(
            status, 200,
            "request {i} after the idle sockets closed: {body}"
        );
    }
    eprintln!(
        "300 idle connections, 300 requests ({refused} refused 503, {served} served) and 20 \
         after: {:?}",
        started.elapsed()
    );
}

#[test]
fn an_idle_connection_is_closed_at_the_read_timeout() {
    // [VLD-ANB-MIDGARD-0004] [AC-31.2]
    let addr = spawn_dev_gateway_with(with_listener(ListenerLimits {
        read_timeout: Duration::from_secs(1),
        ..ListenerLimits::edge()
    }));
    let mut silent = TcpStream::connect(addr).unwrap();
    let (closed, sent) = closed_by_edge(&mut silent, Duration::from_secs(3));
    assert!(
        closed,
        "a connection that sends nothing is closed: {sent:?}"
    );
    assert_healthy(addr);
}

#[test]
fn a_slow_header_sender_is_cut_off_at_the_head_deadline() {
    // [VLD-ANB-MIDGARD-0004] [AC-31.2]
    let addr = spawn_dev_gateway_with(with_listener(ListenerLimits {
        head_deadline: Duration::from_secs(1),
        ..ListenerLimits::edge()
    }));
    let mut slow = TcpStream::connect(addr).unwrap();
    slow.write_all(b"GET /api/v1/health HTTP/1.1\r\n").unwrap();
    slow.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();

    // One byte of a header every 200 ms: each read is quick, the head never ends.
    let started = Instant::now();
    let mut got = Vec::new();
    let closed = loop {
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "still open after {:?}",
            started.elapsed()
        );
        if slow.write_all(b"X").is_err() {
            break true;
        }
        let mut buf = [0u8; 256];
        match slow.read(&mut buf) {
            Ok(0) => break true,
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break true,
        }
    };
    let got = String::from_utf8_lossy(&got);
    assert!(closed && !got.contains("200 OK"), "{got}");
    assert_healthy(addr);
}

#[test]
fn a_source_opening_connections_too_fast_gets_429() {
    // [VLD-ANB-MIDGARD-0004] [AC-31.2]
    let addr = spawn_dev_gateway_with(EdgeLimits {
        source_burst: 5.0,
        source_per_sec: 0.0,
        ..EdgeLimits::default()
    });
    for i in 0..5 {
        let (status, _, body) = get(addr, "/api/v1/health");
        assert_eq!(
            status, 200,
            "connection {i} is within the allowance: {body}"
        );
    }
    let reply = get(addr, "/api/v1/health");
    assert_eq!(
        reply.0, 429,
        "the sixth connection from 127.0.0.1: {}",
        reply.2
    );
    assert_eq!(header(&reply, "Retry-After"), Some("1"));
}
