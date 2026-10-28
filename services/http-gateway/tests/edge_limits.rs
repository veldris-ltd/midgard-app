//! The edge's limits: a request too large, or framed without a length, is refused with
//! a status that says so before its body is read; and repeated wrong passwords lock a
//! username for a while, each lockout recorded in the ledger. [NFR-SEC-1] [RC-37]
#![cfg(feature = "backend-fips")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};

use http_gateway::http::{Request, Response, MAX_BODY_BYTES, MAX_HEAD_BYTES};
use http_gateway::login_throttle::{FIRST_LOCKOUT, FREE_FAILURES};
use http_gateway::{SecuredGateway, DEMO_PASSWORD, DEMO_USERNAME};

const ADDR: &str = "127.0.0.1:17712";
const NOW: u64 = 1_800_000_000;

/// One development edge on [`ADDR`] for every socket test in this file.
fn edge() {
    static START: Once = Once::new();
    START.call_once(|| {
        thread::spawn(|| {
            let _ = http_gateway::serve(ADDR);
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while TcpStream::connect(ADDR).is_err() {
            assert!(Instant::now() < deadline, "the edge did not start");
            thread::sleep(Duration::from_millis(50));
        }
    });
}

/// Send `head` then `body` on a fresh connection; the response, and how long it took.
fn exchange(head: &str, body: &[u8]) -> (String, Duration) {
    edge();
    let start = Instant::now();
    let mut stream = TcpStream::connect(ADDR).expect("connect to the edge");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(head.as_bytes()).unwrap();
    // The edge may answer and stop reading before the body is all sent.
    let _ = stream.write_all(body);
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    (String::from_utf8_lossy(&resp).into_owned(), start.elapsed())
}

fn post_head(length: usize, extra: &str) -> String {
    format!(
        "POST /api/v1/query HTTP/1.1\r\nHost: localhost\r\n\
         Origin: http://localhost:5173\r\nContent-Type: application/json\r\n\
         {extra}Content-Length: {length}\r\nConnection: close\r\n\r\n"
    )
}

#[test]
fn an_oversized_body_is_413_quickly_and_the_edge_stays_up() {
    let size = 5 * 1024 * 1024;
    let (resp, took) = exchange(&post_head(size, ""), &vec![b'x'; size]);
    assert!(resp.starts_with("HTTP/1.1 413 Payload Too Large"), "{resp}");
    assert!(took < Duration::from_secs(3), "took {took:?}");
    // A console on the allowlisted origin can read the refusal.
    assert!(resp.contains("Access-Control-Allow-Origin: http://localhost:5173"));

    // A declared length alone is enough: nothing near it is allocated or awaited.
    let (resp, took) = exchange(&post_head(usize::MAX / 2, ""), b"");
    assert!(resp.starts_with("HTTP/1.1 413"), "{resp}");
    assert!(took < Duration::from_secs(3), "took {took:?}");

    let (health, _) = exchange(
        "GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        b"",
    );
    assert!(health.starts_with("HTTP/1.1 200 OK"), "{health}");
}

#[test]
fn a_body_at_the_limit_is_read_and_served() {
    let json = r#"{"objectType":"aircraft"}"#;
    let body = format!("{json}{}", " ".repeat(MAX_BODY_BYTES - json.len()));
    assert_eq!(body.len(), MAX_BODY_BYTES);
    let (resp, _) = exchange(&post_head(body.len(), ""), body.as_bytes());
    // Read in full and served: refused for want of a token, not for its size.
    assert!(resp.starts_with("HTTP/1.1 401"), "{resp}");
}

#[test]
fn a_chunked_body_or_a_bad_length_is_refused_not_read_as_empty() {
    let chunked = "POST /api/v1/query HTTP/1.1\r\nHost: localhost\r\n\
                   Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
    let (resp, _) = exchange(chunked, b"5\r\nhello\r\n0\r\n\r\n");
    assert!(resp.starts_with("HTTP/1.1 411 Length Required"), "{resp}");

    let bad = "POST /api/v1/query HTTP/1.1\r\nHost: localhost\r\n\
               Content-Length: lots\r\nConnection: close\r\n\r\n";
    let (resp, _) = exchange(bad, b"");
    assert!(resp.starts_with("HTTP/1.1 400"), "{resp}");
}

#[test]
fn an_oversized_head_is_431() {
    let padding = "a".repeat(MAX_HEAD_BYTES);
    let head =
        format!("GET /api/v1/health HTTP/1.1\r\nX-Padding: {padding}\r\nConnection: close\r\n\r\n");
    let (resp, _) = exchange(&head, b"");
    assert!(
        resp.starts_with("HTTP/1.1 431 Request Header Fields Too Large"),
        "{resp}"
    );
}

// ---- failed logins ------------------------------------------------------------

fn login(gw: &SecuredGateway, username: &str, password: &str, at: u64) -> Response {
    let body = format!(r#"{{"username":"{username}","password":"{password}"}}"#);
    gw.handle(
        &Request {
            method: "POST".into(),
            path: "/api/v1/login".into(),
            query: String::new(),
            body,
            authorization: None,
            origin: None,
        },
        at,
    )
}

/// Fail `n` logins for `username`, all at `at`, each a uniform 401.
fn fail(gw: &SecuredGateway, username: &str, n: u32, at: u64) {
    for i in 0..n {
        let r = login(gw, username, "wrong", at);
        assert_eq!(r.status, 401, "failure {i}: {}", r.body);
    }
}

#[test]
fn repeated_wrong_passwords_lock_the_name_even_against_the_right_one() {
    let gw = SecuredGateway::dev();
    fail(&gw, DEMO_USERNAME, FREE_FAILURES, NOW);

    let locking = login(&gw, DEMO_USERNAME, "wrong", NOW);
    assert_eq!(locking.status, 429, "{}", locking.body);
    assert_eq!(locking.retry_after, Some(FIRST_LOCKOUT));

    let right = login(&gw, DEMO_USERNAME, DEMO_PASSWORD, NOW + 1);
    assert_eq!(
        right.status, 429,
        "the lock holds against the right password"
    );
    assert_eq!(right.retry_after, Some(FIRST_LOCKOUT - 1));
    assert!(right.body.contains("try again in"), "{}", right.body);

    // Once it lapses, the right password works and clears the count.
    let after = login(&gw, DEMO_USERNAME, DEMO_PASSWORD, NOW + FIRST_LOCKOUT);
    assert_eq!(after.status, 200, "{}", after.body);
    fail(&gw, DEMO_USERNAME, FREE_FAILURES, NOW + FIRST_LOCKOUT + 1);
}

#[test]
fn an_unknown_name_locks_exactly_as_a_known_one_does() {
    let gw = SecuredGateway::dev();
    fail(&gw, "nobody", FREE_FAILURES, NOW);
    let locked = login(&gw, "nobody", "wrong", NOW);
    assert_eq!(locked.status, 429, "{}", locked.body);
    assert_eq!(locked.retry_after, Some(FIRST_LOCKOUT));

    // One name's lock is not another's.
    let other = login(&gw, DEMO_USERNAME, DEMO_PASSWORD, NOW);
    assert_eq!(other.status, 200, "{}", other.body);
}

#[test]
fn every_throttled_attempt_is_on_the_ledger() {
    let gw = SecuredGateway::dev();
    let before = gw.audit_len();
    fail(&gw, DEMO_USERNAME, FREE_FAILURES, NOW);
    assert_eq!(login(&gw, DEMO_USERNAME, "wrong", NOW).status, 429);
    assert_eq!(login(&gw, DEMO_USERNAME, DEMO_PASSWORD, NOW).status, 429);
    assert_eq!(
        gw.audit_len(),
        before + 7,
        "five refusals, the locking failure, and the throttled attempt"
    );
}
