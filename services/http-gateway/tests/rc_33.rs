//! RC-33 — Given a running service, then it exposes real liveness/readiness +
//! metrics, and SLO breaches are observable; no telemetry egresses the enclosure.
//!
//! The gateway is instrumented with the zero-dependency in-enclosure telemetry stack:
//! `/api/v1/health` (liveness), `/api/v1/ready` (readiness, 503 when a dependency is
//! down), `/metrics` (Prometheus text), request counters + latency, and an
//! availability SLO whose breach is visible in a `/metrics` scrape. Because the
//! telemetry crate has zero dependencies it has no network client and cannot egress.
//! [RC-33] [NFR-PRV-1]
#![cfg(feature = "backend-fips")]

use http_gateway::http::Request;
use http_gateway::SecuredGateway;

const NOW: u64 = 1_000_000;

fn get(gw: &SecuredGateway, path: &str) -> http_gateway::http::Response {
    let req = Request {
        method: "GET".into(),
        path: path.into(),
        query: String::new(),
        body: String::new(),
        authorization: None,
        origin: None,
    };
    gw.handle(&req, NOW)
}

#[test]
fn service_exposes_liveness_readiness_metrics_and_an_observable_slo_breach() {
    let gw = SecuredGateway::dev();

    // Liveness: a 200 proves the process is live.
    let health = get(&gw, "/api/v1/health");
    assert_eq!(health.status, 200);
    assert!(health.body.contains("\"status\":\"ok\""));

    // Readiness: 200 while every dependency is healthy.
    let ready = get(&gw, "/api/v1/ready");
    assert_eq!(ready.status, 200);
    assert!(ready.body.contains("\"ready\":true"));
    assert!(ready.body.contains("\"name\":\"gateway\""));

    // Metrics: real Prometheus text exposition with request counters + the SLO gauges.
    let metrics = get(&gw, "/metrics");
    assert_eq!(metrics.status, 200);
    assert!(metrics.content_type.starts_with("text/plain"));
    assert!(metrics.body.contains("# TYPE http_requests_total counter"));
    assert!(metrics
        .body
        .contains("http_requests_total{path=\"/api/v1/health\",status=\"200\"}"));
    assert!(metrics
        .body
        .contains("slo_availability_ratio{slo=\"gateway_availability\"}"));
    assert!(metrics
        .body
        .contains("slo_breaching{slo=\"gateway_availability\"} 0"));

    // A dependency going down flips readiness to 503 (liveness is unaffected).
    gw.set_ready(false);
    let not_ready = get(&gw, "/api/v1/ready");
    assert_eq!(not_ready.status, 503);
    assert!(not_ready.body.contains("\"ready\":false"));
    assert_eq!(get(&gw, "/api/v1/health").status, 200, "still live");

    // Sustained unreadiness (a stream of 5xx) breaches the availability SLO — and the
    // breach is directly observable in the next /metrics scrape.
    for _ in 0..2000 {
        assert_eq!(get(&gw, "/api/v1/ready").status, 503);
    }
    let breached = get(&gw, "/metrics");
    assert!(
        breached
            .body
            .contains("slo_breaching{slo=\"gateway_availability\"} 1"),
        "the availability SLO breach must be observable in /metrics"
    );
    // The 503s are counted, too.
    assert!(breached
        .body
        .contains("http_requests_total{path=\"/api/v1/ready\",status=\"503\"}"));
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn call(
    gw: &SecuredGateway,
    method: &str,
    path: &str,
    query: &str,
    body: &str,
    token: Option<&str>,
) -> http_gateway::http::Response {
    gw.handle(
        &Request {
            method: method.into(),
            path: path.into(),
            query: query.into(),
            body: body.into(),
            authorization: token.map(|t| format!("Bearer {t}")),
            origin: None,
        },
        now(),
    )
}

// A panic while the SLO lock is held no longer makes every later request panic after
// its ledger row is written: the lock is recovered, the response goes out, and the
// recovery is counted in /metrics once. [VLD-ANB-MIDGARD-0030] [AC-28.4]
#[test]
fn a_poisoned_slo_lock_does_not_drop_responses() {
    let gw = SecuredGateway::dev();
    gw.poison_availability_for_tests();

    assert_eq!(get(&gw, "/api/v1/health").status, 200);
    let metrics = get(&gw, "/metrics");
    assert_eq!(metrics.status, 200);
    assert!(
        metrics
            .body
            .contains("lock_poison_recoveries_total{lock=\"availability\"} 1\n"),
        "{}",
        metrics.body
    );
}

// A panic while the enclosure lock is held is recovered for the next request, which
// is served, and the recovery is recorded in the governance ledger.
// [VLD-ANB-MIDGARD-0030] [AC-28.5]
#[test]
fn a_poisoned_enclosure_lock_is_recovered_logged_and_audited() {
    use http_gateway::{DEMO_PASSWORD, DEMO_USERNAME};

    let gw = SecuredGateway::dev();
    gw.poison_enclosure_for_tests();

    let login = format!(r#"{{"username":"{DEMO_USERNAME}","password":"{DEMO_PASSWORD}"}}"#);
    let resp = call(&gw, "POST", "/api/v1/login", "", &login, None);
    assert_eq!(resp.status, 200, "{}", resp.body);
    let token = http_gateway::json::parse(&resp.body)
        .ok()
        .and_then(|j| j.str_field("token").map(str::to_string))
        .expect("login returns a token");

    let audit = call(&gw, "GET", "/api/v1/audit", "limit=500", "", Some(&token));
    assert_eq!(audit.status, 200, "{}", audit.body);
    assert!(
        audit.body.contains(
            r#""actor":"system","action":"RECOVERED poisoned lock","resource":"lock:enclosure""#
        ),
        "{}",
        audit.body
    );
    assert!(get(&gw, "/metrics")
        .body
        .contains("lock_poison_recoveries_total{lock=\"enclosure\"} 1\n"));
}
