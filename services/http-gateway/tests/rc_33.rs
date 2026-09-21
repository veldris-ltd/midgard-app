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
