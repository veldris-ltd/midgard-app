//! The edge's security regressions, driven over a real socket. Each test starts its own
//! development gateway on a fresh ephemeral port, so no test shares state with another,
//! and speaks raw HTTP to it so a request can be as malformed as an attacker's.
//!
//! This file is the shared harness for the edge security work packages: later ones add
//! their tests here, using [`spawn_dev_gateway`], [`raw_request`], [`post_json`] and
//! [`get`]; [`spawn_dev_gateway_with`] starts one within chosen limits.
//! [VLD-ANB-MIDGARD-0006] [VLD-ANB-MIDGARD-0004] [NFR-SEC-1] [AC-30.1] [AC-30.6]
//! [AC-31.1] [AC-31.2] [VLD-ANB-MIDGARD-0005] [AC-11.1] [AC-11.2] [AC-11.3]
//! [VLD-ANB-MIDGARD-0019] [AC-12.1] [AC-12.2] [AC-12.4]
//! [VLD-ANB-MIDGARD-0020] [VLD-ANB-MIDGARD-0003] [AC-33.1] [AC-33.2]
#![cfg(feature = "backend-fips")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use governance::Governance;
use http_gateway::cli;
use http_gateway::http::{Request, Response, MAX_BODY_BYTES};
use http_gateway::json::{parse, Json};
use http_gateway::login_throttle::{
    LoginThrottle, FREE_FAILURES, LOGIN_SOURCE_BURST, LOGIN_SOURCE_PER_SEC, MAX_PASSWORD_BYTES,
    MAX_TOTP_CHARS, MAX_TRACKED, MAX_USERNAME_BYTES,
};
use http_gateway::{
    demo_operator, dev_login_policy, EdgeLimits, ListenerLimits, SecuredGateway, DEMO_PASSWORD,
    DEMO_USERNAME,
};
use iam::{Idp, Pbkdf2Hasher};
use policy::DefaultPdp;
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

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
    spawn_gateway_with(SecuredGateway::dev(), limits)
}

/// Serve `gw` within `limits` on a fresh loopback port, as [`spawn_dev_gateway`] does.
pub fn spawn_gateway_with(gw: SecuredGateway, limits: EdgeLimits) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("the bound address");
    let gw = Arc::new(gw);
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

// ---- WP-11: login bounds, per-source limit, ledger coalescing --------------------

const NOW: u64 = 1_800_000_000;

/// The development gateway with the default per-source burst but no refill, so a test
/// that takes longer than the refill period still sees the burst end where it should.
fn quota_gateway() -> SecuredGateway {
    SecuredGateway::dev().with_login_source_limit(LOGIN_SOURCE_BURST, 0.0)
}

/// A login request for `username` and `password`, as the edge parses one.
fn login_request(username: &str, password: &str) -> Request {
    Request {
        method: "POST".into(),
        path: "/api/v1/login".into(),
        query: String::new(),
        body: format!(r#"{{"username":"{username}","password":"{password}"}}"#),
        authorization: None,
        origin: None,
        forwarded_for: None,
    }
}

/// `n` wrong logins from `peer`, each under a name of its own (a password spray): the
/// responses in order.
fn spray(gw: &SecuredGateway, peer: SocketAddr, n: usize, at: u64) -> Vec<Response> {
    (0..n)
        .map(|i| {
            let name = format!("spray-{}-{at}-{i}", peer.ip());
            gw.handle_from(&login_request(&name, "wrong"), Some(peer), at)
        })
        .collect()
}

/// The actions of the newest `limit` ledger rows, read in process as the demo operator
/// (the read itself adds rows, so take any count first).
fn ledger_actions(gw: &SecuredGateway, limit: usize) -> Vec<String> {
    let login = gw.handle(&login_request(DEMO_USERNAME, DEMO_PASSWORD), NOW);
    assert_eq!(login.status, 200, "{}", login.body);
    let token = parse(&login.body)
        .ok()
        .and_then(|j| j.str_field("token").map(str::to_string))
        .expect("login returns a token");
    let audit = gw.handle(
        &Request {
            method: "GET".into(),
            path: "/api/v1/audit".into(),
            query: format!("limit={limit}"),
            body: String::new(),
            authorization: Some(format!("Bearer {token}")),
            origin: None,
            forwarded_for: None,
        },
        NOW,
    );
    assert_eq!(audit.status, 200, "{}", audit.body);
    match parse(&audit.body).expect("the audit view is JSON") {
        Json::Arr(rows) => rows
            .iter()
            .filter_map(|row| row.str_field("action").map(str::to_string))
            .collect(),
        other => panic!("the audit view is an array, not {other:?}"),
    }
}

#[test]
fn a_one_mib_username_is_refused_before_any_state_is_touched() {
    // [VLD-ANB-MIDGARD-0005] [AC-11.1] [AC-11.5]
    let gw = SecuredGateway::dev();
    let before = gw.audit_len();
    let huge = "a".repeat(1_000_000);
    let reply = gw.handle(&login_request(&huge, "x"), NOW);
    assert_eq!(reply.status, 400, "{}", reply.body);
    assert!(
        reply.body.contains("login fields exceed their limits"),
        "{}",
        reply.body
    );
    assert_eq!(gw.audit_len(), before, "the ledger has no new row");
    assert_eq!(
        gw.login_throttle_stats(),
        (0, 0),
        "the throttle has no new entry"
    );

    // A password or one-time code over its limit is refused the same way.
    let long_password = "p".repeat(MAX_PASSWORD_BYTES + 1);
    let reply = gw.handle(&login_request(DEMO_USERNAME, &long_password), NOW);
    assert_eq!(reply.status, 400, "{}", reply.body);
    let mut long_code = login_request(DEMO_USERNAME, DEMO_PASSWORD);
    long_code.body = format!(
        r#"{{"username":"{DEMO_USERNAME}","password":"{DEMO_PASSWORD}","totp":"{}"}}"#,
        "1".repeat(MAX_TOTP_CHARS + 1)
    );
    assert_eq!(gw.handle(&long_code, NOW).status, 400);
    assert_eq!(gw.audit_len(), before);

    // A name at the limit is still an ordinary login.
    let at_limit = "a".repeat(MAX_USERNAME_BYTES);
    assert_eq!(gw.handle(&login_request(&at_limit, "x"), NOW).status, 401);

    let addr = spawn_dev_gateway();
    let body = format!(r#"{{"username":"{huge}","password":"x"}}"#);
    let (status, _, reply) = post_json(addr, "/api/v1/login", &body);
    assert_eq!(status, 400, "{reply}");
    assert_healthy(addr);
}

#[test]
fn a_username_with_control_characters_is_400() {
    // [VLD-ANB-MIDGARD-0005] [AC-11.1]
    let gw = SecuredGateway::dev();
    let before = gw.audit_len();
    for name in [
        r"oli\nvia",
        r"oli\u0007via",
        r"\u001b[2Jolivia",
        r"olivia\u0000",
    ] {
        let reply = gw.handle(&login_request(name, "x"), NOW);
        assert_eq!(reply.status, 400, "{name}: {}", reply.body);
    }
    assert_eq!(gw.audit_len(), before, "the ledger has no new row");
    assert_eq!(
        gw.login_throttle_stats(),
        (0, 0),
        "the throttle has no new entry"
    );
}

#[test]
fn ten_thousand_distinct_names_hold_under_two_mib() {
    // [VLD-ANB-MIDGARD-0005] [AC-11.2]
    let name = |i: usize| format!("{i:0>width$}", width = MAX_USERNAME_BYTES);

    // Through the edge, a failed login keeps its name and nothing more: no name longer
    // than the limit gets this far.
    let gw = SecuredGateway::dev();
    for i in 0..20 {
        let reply = gw.handle(&login_request(&name(i), "wrong"), NOW);
        assert_eq!(reply.status, 401, "failure {i}: {}", reply.body);
    }
    assert_eq!(gw.login_throttle_stats(), (20, 20 * MAX_USERNAME_BYTES));

    // Every login through the edge costs a password check and a signed ledger row, too
    // slow for ten thousand in a debug build, so the throttle the edge holds is filled
    // directly with the most it can be given: ten thousand and more limit-length names
    // failing at the same instant.
    let mut throttle = LoginThrottle::new();
    for i in 0..MAX_TRACKED + 50 {
        throttle.record_failure(&name(i), NOW);
    }
    let (tracked, key_bytes) = (throttle.tracked(), throttle.key_bytes());
    eprintln!("{tracked} names tracked in {key_bytes} key bytes");
    assert!(tracked <= MAX_TRACKED, "{tracked} names tracked");
    assert!(key_bytes <= 1_300_000, "{key_bytes} key bytes");
}

#[test]
fn a_flooding_source_is_refused_before_verification() {
    // [VLD-ANB-MIDGARD-0005] [AC-11.3]
    let gw = quota_gateway();
    let before = gw.audit_len();
    let flooding: SocketAddr = "10.0.0.9:5000".parse().unwrap();
    let replies = spray(&gw, flooding, 25, NOW);
    for (i, reply) in replies.iter().enumerate() {
        if i < 20 {
            assert_eq!(
                reply.status, 401,
                "attempt {i} is within the burst: {}",
                reply.body
            );
        } else {
            assert_eq!(
                reply.status, 429,
                "attempt {i} is past the burst: {}",
                reply.body
            );
            assert_eq!(reply.retry_after, Some(2));
            assert!(reply.body.contains("from this address"), "{}", reply.body);
        }
    }
    assert_eq!(
        gw.audit_len(),
        before + 21,
        "twenty refusals and one coalesced source-throttled row"
    );
    assert_eq!(
        gw.login_throttle_stats().0,
        20,
        "the refused attempts never reached a password check"
    );

    let other: SocketAddr = "10.0.0.10:5000".parse().unwrap();
    let reply = gw.handle_from(&login_request("someone", "wrong"), Some(other), NOW);
    assert_eq!(
        reply.status, 401,
        "another address is unaffected: {}",
        reply.body
    );
    let reply = gw.handle(&login_request(DEMO_USERNAME, DEMO_PASSWORD), NOW);
    assert_eq!(reply.status, 200, "a correct password still yields a token");
}

/// The operator's reverse proxy, as `--trusted-proxy` names it.
const PROXY: &str = "10.0.0.1";

/// `n` wrong logins arriving from `peer`, each under a name of its own and the
/// `X-Forwarded-For` that `forwarded(i)` gives: the responses in order.
fn spray_forwarded(
    gw: &SecuredGateway,
    peer: SocketAddr,
    forwarded: impl Fn(usize) -> String,
    n: usize,
) -> Vec<Response> {
    (0..n)
        .map(|i| {
            let mut request = login_request(&format!("spray-fwd-{i}"), "wrong");
            request.forwarded_for = Some(forwarded(i));
            gw.handle_from(&request, Some(peer), NOW)
        })
        .collect()
}

#[test]
fn clients_behind_a_trusted_proxy_draw_on_separate_allowances() {
    // [VLD-ANB-MIDGARD-0005] [AC-11.3]
    let gw = quota_gateway().with_trusted_proxies([PROXY.parse().unwrap()]);
    let proxy: SocketAddr = format!("{PROXY}:5000").parse().unwrap();
    // The client writes what it likes on the left; the proxy appends the address it
    // was reached from, and only that is believed.
    let replies = spray_forwarded(&gw, proxy, |i| format!("203.0.113.{i}, 198.51.100.7"), 25);
    for (i, reply) in replies.iter().enumerate() {
        let expected = if i < 20 { 401 } else { 429 };
        assert_eq!(reply.status, expected, "attempt {i}: {}", reply.body);
    }
    let actions = ledger_actions(&gw, 10);
    assert!(
        actions.iter().any(|a| a == "LOGIN source throttled"),
        "{actions:?}"
    );

    // Another client behind the same proxy still has its whole allowance, and so does
    // one the proxy reached through a second trusted hop.
    for forwarded in ["198.51.100.8", "198.51.100.9, 10.0.0.1"] {
        let replies = spray_forwarded(&gw, proxy, |_| forwarded.to_string(), 20);
        assert!(
            replies.iter().all(|r| r.status == 401),
            "{forwarded}: {:?}",
            replies.iter().map(|r| r.status).collect::<Vec<_>>()
        );
    }
}

#[test]
fn a_forwarded_for_from_an_untrusted_peer_is_ignored() {
    // [VLD-ANB-MIDGARD-0005] [AC-11.3]
    let gw = quota_gateway().with_trusted_proxies([PROXY.parse().unwrap()]);
    let direct: SocketAddr = "10.0.0.9:5000".parse().unwrap();
    // A new forged client every time: the bucket is still the peer's own.
    let replies = spray_forwarded(&gw, direct, |i| format!("198.51.100.{i}"), 25);
    for (i, reply) in replies.iter().enumerate() {
        let expected = if i < 20 { 401 } else { 429 };
        assert_eq!(reply.status, expected, "attempt {i}: {}", reply.body);
    }

    // From the trusted proxy, an entry that is not an address stops the walk: what
    // is left of it is the client's, so the attempt is charged to the proxy.
    let proxy: SocketAddr = format!("{PROXY}:5000").parse().unwrap();
    let replies = spray_forwarded(&gw, proxy, |i| format!("198.51.100.{i}, not-an-ip"), 25);
    assert_eq!(replies[20].status, 429, "{}", replies[20].body);
}

#[test]
fn a_source_throttle_is_recorded_once_per_minute() {
    // [VLD-ANB-MIDGARD-0005] [AC-11.3]
    let gw = quota_gateway();
    let flooding: SocketAddr = "10.0.0.9:5000".parse().unwrap();
    spray(&gw, flooding, 25, NOW);
    let before = gw.audit_len();
    let reply = gw.handle_from(&login_request("late", "wrong"), Some(flooding), NOW + 30);
    assert_eq!(reply.status, 429);
    assert_eq!(
        gw.audit_len(),
        before,
        "within the minute: counted, not recorded"
    );

    let reply = gw.handle_from(&login_request("later", "wrong"), Some(flooding), NOW + 61);
    assert_eq!(reply.status, 429);
    assert_eq!(gw.audit_len(), before + 1, "a minute on: one more row");
    let actions = ledger_actions(&gw, 10);
    assert!(
        actions
            .iter()
            .any(|a| a == "LOGIN source throttled, 5 attempts suppressed"),
        "the row names the suppressed count: {actions:?}"
    );
}

#[test]
fn source_throttling_applies_to_admin_routes() {
    // [VLD-ANB-MIDGARD-0005] [AC-11.3]
    let gw = quota_gateway();
    let peer: SocketAddr = "10.0.0.9:5000".parse().unwrap();
    let challenge = Request {
        method: "POST".into(),
        path: "/api/v1/admin/challenge".into(),
        query: String::new(),
        body: String::new(),
        authorization: None,
        origin: None,
        forwarded_for: None,
    };
    for i in 0..25 {
        let reply = gw.handle_from(&challenge, Some(peer), NOW);
        let expected = if i < 20 { 404 } else { 429 };
        assert_eq!(reply.status, expected, "attempt {i}: {}", reply.body);
    }
    // The address's login attempts draw on the same allowance.
    let reply = gw.handle_from(&login_request("someone", "wrong"), Some(peer), NOW);
    assert_eq!(reply.status, 429, "{}", reply.body);
}

/// An activation for `username` with `code`, under a password long enough to keep.
fn activate_request(username: &str, code: &str) -> Request {
    Request {
        method: "POST".into(),
        path: "/api/v1/activate".into(),
        query: String::new(),
        body: format!(
            r#"{{"username":"{username}","activationCode":"{code}","password":"long enough to keep"}}"#
        ),
        authorization: None,
        origin: None,
        forwarded_for: None,
    }
}

// ---- NEW-8: Argon2id runs without the directory lock --------------------------------

/// The activation code every activation test provisions.
const ACTIVATION_CODE: &str = "0123456789abcdef0123456789abcdef";

/// A development gateway with no per-source limit that runs `hook` once, between the
/// first login's or activation's check and its commit.
fn gateway_with_hook(
    mut gw: SecuredGateway,
    hook: impl Fn(&SecuredGateway) + Send + Sync + 'static,
) -> SecuredGateway {
    gw = gw.with_login_source_limit(0.0, 0.0);
    let once = AtomicBool::new(false);
    gw.with_check_commit_hook(move |gw| {
        if !once.swap(true, Ordering::SeqCst) {
            hook(gw)
        }
    })
}

#[test]
fn a_password_changed_while_a_login_is_checked_does_not_log_in_with_the_old_one() {
    // [NFR-SEC-1] [VLD-ANB-MIDGARD-0019] The hook takes the directory lock itself, so
    // the check cannot still hold it: if it did, the login would never return.
    let gw = gateway_with_hook(SecuredGateway::dev(), |gw| {
        assert!(gw.reset_login_password(DEMO_USERNAME, "a different password"));
    });
    let reply = gw.handle(&login_request(DEMO_USERNAME, DEMO_PASSWORD), NOW);
    assert_eq!(reply.status, 401, "{}", reply.body);
    assert_eq!(
        reply.body, r#"{"error":"invalid credentials"}"#,
        "the uniform 401"
    );

    let reply = gw.handle(&login_request(DEMO_USERNAME, DEMO_PASSWORD), NOW);
    assert_eq!(
        reply.status, 401,
        "the old password is gone: {}",
        reply.body
    );
    let reply = gw.handle(&login_request(DEMO_USERNAME, "a different password"), NOW);
    assert_eq!(reply.status, 200, "the new one logs in: {}", reply.body);
}

#[test]
fn a_principal_removed_while_their_activation_is_checked_is_not_activated() {
    // [NFR-SEC-1] [VLD-ANB-MIDGARD-0002] [VLD-ANB-MIDGARD-0019]
    let pending = || {
        let mut gw = SecuredGateway::dev();
        gw.provision_activation("olivia", ACTIVATION_CODE, NOW + 3_600, demo_operator())
            .expect("provisions in memory");
        gw
    };
    let gw = gateway_with_hook(pending(), |gw| assert!(gw.remove_login("olivia")));
    let reply = gw.handle(&activate_request("olivia", ACTIVATION_CODE), NOW);
    assert_eq!(reply.status, 401, "{}", reply.body);
    let reply = gw.handle(&login_request("olivia", "long enough to keep"), NOW);
    assert_eq!(reply.status, 401, "{}", reply.body);

    // Spent in the gap by another activation: the second commit is refused.
    let gw = gateway_with_hook(pending(), |gw| {
        let reply = gw.handle(&activate_request("olivia", ACTIVATION_CODE), NOW);
        assert_eq!(reply.status, 200, "{}", reply.body);
    });
    let reply = gw.handle(&activate_request("olivia", ACTIVATION_CODE), NOW);
    assert_eq!(
        reply.status, 401,
        "the code was spent meanwhile: {}",
        reply.body
    );

    // Left alone, the same activation succeeds and the password logs in.
    let gw = gateway_with_hook(pending(), |_| {});
    let reply = gw.handle(&activate_request("olivia", ACTIVATION_CODE), NOW);
    assert_eq!(reply.status, 200, "{}", reply.body);
    let reply = gw.handle(&login_request("olivia", "long enough to keep"), NOW);
    assert_eq!(reply.status, 200, "{}", reply.body);
}

#[test]
fn source_throttling_applies_to_activation() {
    // [VLD-ANB-MIDGARD-0005] [AC-11.3]
    let gw = quota_gateway();
    let peer: SocketAddr = "10.0.0.9:5000".parse().unwrap();
    for i in 0..25 {
        let reply = gw.handle_from(
            &activate_request(&format!("spray-{i}"), "00"),
            Some(peer),
            NOW,
        );
        let expected = if i < 20 { 401 } else { 429 };
        assert_eq!(reply.status, expected, "attempt {i}: {}", reply.body);
    }
    assert_eq!(
        gw.login_throttle_stats().0,
        20,
        "the refused attempts never reached the throttle or a code check"
    );
    // The address's login attempts draw on the same allowance.
    let reply = gw.handle_from(&login_request("someone", "wrong"), Some(peer), NOW);
    assert_eq!(reply.status, 429, "{}", reply.body);
    let other: SocketAddr = "10.0.0.10:5000".parse().unwrap();
    let reply = gw.handle_from(&activate_request("someone", "00"), Some(other), NOW);
    assert_eq!(
        reply.status, 401,
        "another address is unaffected: {}",
        reply.body
    );
}

#[test]
fn activation_fields_are_bounded_before_the_throttle_or_the_ledger() {
    // [VLD-ANB-MIDGARD-0005] [AC-11.1]
    let gw = quota_gateway();
    let before = (gw.audit_len(), gw.login_throttle_stats());
    let long_name = "n".repeat(MAX_USERNAME_BYTES + 1);
    let long_code = "0".repeat(65);
    for (what, request) in [
        ("a long name", activate_request(&long_name, "00")),
        (
            "a name with a control character",
            activate_request(r"ol\u0007ivia", "00"),
        ),
        ("a long code", activate_request("olivia", &long_code)),
    ] {
        let reply = gw.handle(&request, NOW);
        assert_eq!(reply.status, 400, "{what}: {}", reply.body);
        assert!(
            reply.body.contains("exceed their limits"),
            "{what}: {}",
            reply.body
        );
    }
    let long_password = Request {
        body: format!(
            r#"{{"username":"olivia","activationCode":"00","password":"{}"}}"#,
            "p".repeat(MAX_PASSWORD_BYTES + 1)
        ),
        ..activate_request("olivia", "00")
    };
    assert_eq!(
        gw.handle(&long_password, NOW).status,
        400,
        "a long password"
    );
    assert_eq!(
        (gw.audit_len(), gw.login_throttle_stats()),
        before,
        "nothing was kept, tracked or recorded"
    );
    let at_limit = "n".repeat(MAX_USERNAME_BYTES);
    let reply = gw.handle(&activate_request(&at_limit, "00"), NOW);
    assert_eq!(
        reply.status, 401,
        "a name at the limit is checked: {}",
        reply.body
    );
}

#[test]
fn the_login_limit_flags_are_parsed() {
    // [VLD-ANB-MIDGARD-0005]
    let args = |extra: &[&str]| {
        let mut argv = vec!["--state-dir".to_string(), "/state".to_string()];
        argv.extend(extra.iter().map(|s| s.to_string()));
        cli::parse(&argv)
    };
    let Ok(cli::Command::Serve(serve)) = args(&[]) else {
        panic!("a state directory serves");
    };
    assert_eq!(
        (serve.login_burst, serve.login_per_sec),
        (LOGIN_SOURCE_BURST, LOGIN_SOURCE_PER_SEC)
    );
    let Ok(cli::Command::Serve(serve)) = args(&["--login-burst", "50", "--login-per-sec", "2"])
    else {
        panic!("the login limits are options");
    };
    assert_eq!((serve.login_burst, serve.login_per_sec), (50.0, 2.0));
    assert!(args(&["--login-burst", "many"]).is_err());
    assert!(args(&["--login-per-sec", "-1"]).is_err());
}

#[test]
fn the_trusted_proxy_flag_is_repeatable_and_takes_addresses() {
    // [VLD-ANB-MIDGARD-0005]
    let args = |extra: &[&str]| {
        let mut argv = vec!["--state-dir".to_string(), "/state".to_string()];
        argv.extend(extra.iter().map(|s| s.to_string()));
        cli::parse(&argv)
    };
    let Ok(cli::Command::Serve(serve)) = args(&[]) else {
        panic!("a state directory serves");
    };
    assert!(
        serve.trusted_proxies.is_empty(),
        "no proxy is trusted unless named"
    );
    let Ok(cli::Command::Serve(serve)) =
        args(&["--trusted-proxy", "10.0.0.1", "--trusted-proxy", "fd00::1"])
    else {
        panic!("--trusted-proxy is an option");
    };
    assert_eq!(
        serve.trusted_proxies,
        vec![
            "10.0.0.1".parse::<std::net::IpAddr>().unwrap(),
            "fd00::1".parse().unwrap()
        ]
    );
    assert!(args(&["--trusted-proxy", "proxy.internal"]).is_err());
    assert!(args(&["--trusted-proxy", "10.0.0.0/8"]).is_err());
}

// ---- WP-12: the lockout reserves attempts, and a flood cannot clear it ------------

/// A gateway with no per-source limit on connections or logins, so every refusal a
/// test sees is the name's own.
fn unlimited_edge() -> SocketAddr {
    spawn_gateway_with(
        SecuredGateway::dev().with_login_source_limit(0.0, 0.0),
        EdgeLimits {
            source_burst: 0.0,
            source_per_sec: 0.0,
            ..EdgeLimits::default()
        },
    )
}

fn demo_login(password: &str) -> String {
    format!(r#"{{"username":"{DEMO_USERNAME}","password":"{password}"}}"#)
}

/// `n` wrong-password logins for the demo operator, all sent at the same moment, each
/// on its own connection: every reply.
fn simultaneous_wrong_logins(addr: SocketAddr, n: usize) -> Vec<Reply> {
    let start = Arc::new(Barrier::new(n));
    let threads: Vec<_> = (0..n)
        .map(|_| {
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                post_json(addr, "/api/v1/login", &demo_login("wrong"))
            })
        })
        .collect();
    threads.into_iter().map(|t| t.join().unwrap()).collect()
}

#[test]
fn parallel_wrong_passwords_cannot_exceed_the_free_failures() {
    // [VLD-ANB-MIDGARD-0019] [AC-12.1]
    let addr = unlimited_edge();
    let start = Arc::new(Barrier::new(32));
    // Each client keeps trying while it is told the name is busy, as a console told to
    // retry would, until its password is checked or the name is locked.
    let threads: Vec<_> = (0..32)
        .map(|_| {
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                let mut busy = 0;
                loop {
                    let reply = post_json(addr, "/api/v1/login", &demo_login("wrong"));
                    if reply.0 == 429 && reply.2.contains("already being checked") && busy < 500 {
                        busy += 1;
                        thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                    return (reply, busy);
                }
            })
        })
        .collect();
    let replies: Vec<(Reply, u32)> = threads.into_iter().map(|t| t.join().unwrap()).collect();

    let unauthorized = replies.iter().filter(|(r, _)| r.0 == 401).count();
    let locked = replies
        .iter()
        .filter(|(r, _)| r.0 == 429 && r.2.contains("too many failed logins"))
        .count();
    let busy: u32 = replies.iter().map(|(_, busy)| busy).sum();
    eprintln!("32 clients: {unauthorized} x 401, {locked} x 429 locked, {busy} busy refusals");
    assert!(
        unauthorized <= FREE_FAILURES as usize,
        "{unauthorized} passwords refused as wrong before the lock"
    );
    assert_eq!(
        unauthorized + locked,
        32,
        "every other reply is the lock's 429"
    );

    let (status, headers, body) = post_json(addr, "/api/v1/login", &demo_login(DEMO_PASSWORD));
    assert_eq!(status, 429, "the name is locked: {body}");
    assert!(body.contains("too many failed logins"), "{body}");
    assert!(headers.iter().any(|(k, _)| k == "Retry-After"));
}

#[test]
fn a_busy_name_is_429_quickly() {
    // [VLD-ANB-MIDGARD-0019] [AC-12.4] Logins share one directory lock, so attempts
    // that arrive together wait on it holding their reservations: past MAX_IN_FLIGHT
    // the rest are refused at once, before their passwords are checked.
    let addr = unlimited_edge();
    let replies = simultaneous_wrong_logins(addr, 8);
    let busy: Vec<_> = replies
        .iter()
        .filter(|(status, _, body)| *status == 429 && body.contains("already being checked"))
        .collect();
    assert!(!busy.is_empty(), "{replies:?}");
    for reply in &busy {
        assert_eq!(header(reply, "Retry-After"), Some("1"));
    }
    let unauthorized = replies.iter().filter(|r| r.0 == 401).count();
    assert!(unauthorized <= 4, "{unauthorized} checked at once");
    assert_healthy(addr);
}

/// The demo operator's login on a gateway whose password hashing costs next to
/// nothing, so a test can make more logins than a debug build checks at the
/// development work factor in reasonable time.
fn cheap_login_gateway() -> SecuredGateway {
    let idp = Idp::generate("test-idp", &mut OsRng).expect("idp");
    let issuer_vk = idp.verifying_key().clone();
    let governance =
        Governance::new(DefaultPdp::new(dev_login_policy()), &mut OsRng).expect("governance");
    let mut gw = SecuredGateway::new(fips_provider(), issuer_vk, governance, Vec::new())
        .with_idp(idp, Pbkdf2Hasher::new(1));
    gw.provision_login(DEMO_USERNAME, DEMO_PASSWORD, demo_operator())
        .expect("the operator provisions");
    gw
}

// Every login through the edge writes a signed ledger row, about 35 ms in a debug
// build, so twelve thousand take some seven minutes: run it with `--ignored`. The
// throttle's own 20,000-name flood (login_throttle::tests::
// a_locked_name_survives_a_flood_of_new_names) runs every time.
#[test]
#[ignore = "twelve thousand signed ledger rows: about seven minutes in a debug build"]
fn a_flood_of_names_does_not_unlock_a_locked_operator() {
    // [VLD-ANB-MIDGARD-0019] [AC-12.2] Lock the operator, then fail more distinct
    // names than the throttle tracks, each from an address of its own so the
    // per-source bucket lets every one through to a password check.
    let gw = cheap_login_gateway();
    let operator: SocketAddr = "10.255.255.1:5000".parse().unwrap();
    for _ in 0..=FREE_FAILURES {
        gw.handle_from(&login_request(DEMO_USERNAME, "wrong"), Some(operator), NOW);
    }
    let locked = gw.handle(&login_request(DEMO_USERNAME, DEMO_PASSWORD), NOW);
    assert_eq!(locked.status, 429, "{}", locked.body);

    for i in 0..MAX_TRACKED + 2_000 {
        let peer = SocketAddr::from(([10, (i >> 16) as u8, (i >> 8) as u8, i as u8], 5000));
        let reply = gw.handle_from(
            &login_request(&format!("guess-{i}"), "wrong"),
            Some(peer),
            NOW,
        );
        assert_eq!(reply.status, 401, "guess {i}: {}", reply.body);
    }
    assert!(gw.login_throttle_stats().0 <= MAX_TRACKED);

    let after = gw.handle(&login_request(DEMO_USERNAME, DEMO_PASSWORD), NOW + 1);
    assert_eq!(
        after.status, 429,
        "the lock survived the flood: {}",
        after.body
    );
    assert!(
        after.body.contains("too many failed logins"),
        "{}",
        after.body
    );
}

// ---- WP-16: the administrator challenge body ---------------------------------------

#[test]
fn admin_challenge_without_administration_is_still_404_not_a_parse_hole() {
    // [VLD-ANB-MIDGARD-0020] [VLD-ANB-MIDGARD-0006]
    let addr = spawn_dev_gateway();
    let (status, _, body) = post_json(addr, "/api/v1/admin/challenge", &"[".repeat(100_000));
    assert!(status == 404 || status == 400, "{status}: {body}");
    assert_healthy(addr);
}

// ---- WP-33: metrics cardinality --------------------------------------------------

/// The edge's limits with the per-source allowance off: every request in a test
/// comes from 127.0.0.1, so it would answer 429 long before the test's last request.
fn without_source_limit() -> EdgeLimits {
    EdgeLimits {
        source_burst: 0.0,
        source_per_sec: 0.0,
        ..EdgeLimits::default()
    }
}

#[test]
fn unknown_paths_share_one_metrics_series() {
    // [VLD-ANB-MIDGARD-0003] [AC-33.1]
    // Without a bearer token an unknown path is refused before routing, so each of
    // these is a 401, not a 404: the series asserted below is the 401 one.
    let addr = spawn_dev_gateway_with(without_source_limit());
    for i in 0..2000 {
        let (status, _, body) = get(addr, &format!("/probe/{i}"));
        assert_eq!(status, 401, "/probe/{i}: {body}");
    }

    let (status, _, metrics) = get(addr, "/metrics");
    assert_eq!(status, 200);
    let series: Vec<&str> = metrics
        .lines()
        .filter(|l| l.starts_with("http_requests_total{path=\"other\",status=\"401\"}"))
        .collect();
    assert_eq!(
        series,
        ["http_requests_total{path=\"other\",status=\"401\"} 2000"]
    );
    assert!(!metrics.contains("/probe/"), "{metrics}");
    assert!(
        metrics.len() < 64 * 1024,
        "/metrics is {} bytes",
        metrics.len()
    );
    eprintln!(
        "/metrics after 2,000 distinct paths: {} bytes",
        metrics.len()
    );

    // Signed in, an unknown path is a 404, and it shares one series as well.
    let token = demo_token(addr);
    for i in 0..5 {
        let req = format!(
            "GET /probe/signed-in/{i} HTTP/1.1\r\nHost: localhost\r\n\
             Authorization: Bearer {token}\r\nConnection: close\r\n\r\n"
        );
        let (status, _, body) = raw_request(addr, req.as_bytes());
        assert_eq!(status, 404, "{body}");
    }
    let (_, _, metrics) = get(addr, "/metrics");
    assert!(
        metrics.contains("http_requests_total{path=\"other\",status=\"404\"} 5\n"),
        "{metrics}"
    );
    assert!(!metrics.contains("/probe/"), "{metrics}");
}

#[test]
fn a_sixteen_kib_path_leaves_no_trace_in_metrics() {
    // [VLD-ANB-MIDGARD-0003] [AC-33.1]
    let addr = spawn_dev_gateway();
    let path = format!("/{}", "a".repeat(15_000));
    let (status, _, body) = get(addr, &path);
    assert!(matches!(status, 401 | 404 | 431), "{status}: {body}");

    let (status, _, metrics) = get(addr, "/metrics");
    assert_eq!(status, 200);
    assert!(!metrics.contains("aaaa"), "the path is in /metrics");
}

#[test]
fn approval_ids_are_collapsed() {
    // [VLD-ANB-MIDGARD-0003] [AC-33.2]
    let gw = SecuredGateway::dev();
    let request = |method: &str, path: &str| Request {
        method: method.into(),
        path: path.into(),
        query: String::new(),
        body: String::new(),
        authorization: None,
        origin: None,
        forwarded_for: None,
    };
    let decide = gw.handle(&request("POST", "/api/v1/approvals/abc-123/approve"), 0);
    assert_eq!(decide.status, 401, "{}", decide.body);

    let metrics = gw.handle(&request("GET", "/metrics"), 0);
    assert_eq!(metrics.status, 200);
    assert!(
        metrics.body.contains(
            "http_requests_total{path=\"/api/v1/approvals/{id}/approve\",status=\"401\"} 1"
        ),
        "{}",
        metrics.body
    );
    assert!(!metrics.body.contains("abc-123"), "{}", metrics.body);
}
