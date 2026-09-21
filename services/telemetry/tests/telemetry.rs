//! Unit coverage for the telemetry pillars: metrics rendering, health probes,
//! SLO breach detection, structured logging, and trace propagation. [RC-33]

use telemetry::{
    AvailabilitySlo, BufferExporter, BufferSink, HealthRegistry, IdGenerator, LatencySlo, Level,
    Logger, MetricsRegistry, SpanContext, SpanExporter, SpanRecord, Toggle,
};

#[test]
fn metrics_render_prometheus_text() {
    let m = MetricsRegistry::new();
    m.describe("http_requests_total", "total HTTP requests");
    m.inc("http_requests_total", &[("method", "GET"), ("code", "200")]);
    m.inc("http_requests_total", &[("method", "GET"), ("code", "200")]);
    m.inc(
        "http_requests_total",
        &[("method", "POST"), ("code", "403")],
    );
    m.set_gauge("ready", &[], 1.0);
    m.observe("request_latency_ms", &[("route", "query")], 42.0);
    m.observe("request_latency_ms", &[("route", "query")], 700.0);

    let text = m.render();
    assert!(text.contains("# TYPE http_requests_total counter"));
    assert!(text.contains("http_requests_total{code=\"200\",method=\"GET\"} 2"));
    assert!(text.contains("http_requests_total{code=\"403\",method=\"POST\"} 1"));
    assert!(text.contains("# TYPE ready gauge"));
    assert!(text.contains("ready 1"));
    // Histogram: buckets (le cumulative), sum, count.
    assert!(text.contains("# TYPE request_latency_ms histogram"));
    assert!(text.contains("request_latency_ms_bucket{route=\"query\",le=\"50\"} 1"));
    assert!(text.contains("request_latency_ms_bucket{route=\"query\",le=\"1000\"} 2"));
    assert!(text.contains("request_latency_ms_bucket{route=\"query\",le=\"+Inf\"} 2"));
    assert!(text.contains("request_latency_ms_count{route=\"query\"} 2"));
    assert!(text.contains("request_latency_ms_sum{route=\"query\"} 742"));
}

#[test]
fn health_liveness_and_readiness() {
    let mut h = HealthRegistry::new();
    let kms = Box::new(Toggle::new("kms", true));
    let store = Toggle::new("store", true);
    // Keep a handle to flip `store` at runtime.
    let store_ref = std::sync::Arc::new(store);
    h.register(kms);
    h.register(Box::new(ToggleArc(store_ref.clone())));

    // Live and ready.
    assert!(h.liveness().live);
    assert!(h.readiness().ready);

    // A dependency going unhealthy flips readiness (but liveness stays up).
    store_ref.set(false);
    assert!(h.liveness().live);
    assert!(!h.readiness().ready);
    let report = h.readiness();
    assert!(report.to_json().contains("\"ready\":false"));
    assert!(report
        .checks
        .iter()
        .any(|c| c.name == "store" && !c.healthy));
}

/// Wrap an `Arc<Toggle>` so the test can flip it after registration.
struct ToggleArc(std::sync::Arc<Toggle>);
impl telemetry::ReadinessCheck for ToggleArc {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn check(&self) -> bool {
        self.0.check()
    }
    fn detail(&self) -> String {
        self.0.detail()
    }
}

#[test]
fn availability_slo_breach_is_observable_in_metrics() {
    let m = MetricsRegistry::new();
    let mut slo = AvailabilitySlo::new("gateway_availability", 0.99);
    // 98 ok / 100 → 0.98 < 0.99 objective → breaching.
    for _ in 0..98 {
        slo.record(true);
    }
    for _ in 0..2 {
        slo.record(false);
    }
    assert!(slo.is_breaching());
    slo.export(&m);
    let text = m.render();
    assert!(text.contains("slo_availability_ratio{slo=\"gateway_availability\"} 0.98"));
    // The breach is directly observable in a scrape.
    assert!(text.contains("slo_breaching{slo=\"gateway_availability\"} 1"));
}

#[test]
fn first_token_latency_slo_tracks_and_breaches() {
    let mut slo = LatencySlo::new("mimir_first_token", 500.0, 0.99);
    // 95 fast, 5 slow → 0.95 attainment < 0.99 → breaching (tail blew the budget).
    for _ in 0..95 {
        slo.observe(120.0);
    }
    for _ in 0..5 {
        slo.observe(900.0);
    }
    assert!((slo.attainment() - 0.95).abs() < 1e-9);
    assert!(slo.is_breaching());
    assert!(slo.max_ms() >= 900.0);

    let m = MetricsRegistry::new();
    slo.export(&m);
    let text = m.render();
    assert!(text.contains("slo_latency_attainment_ratio{slo=\"mimir_first_token\"} 0.95"));
    assert!(text.contains("slo_breaching{slo=\"mimir_first_token\"} 1"));
}

#[test]
fn structured_logging_carries_trace_context() {
    let sink = BufferSink::new();
    let logger = Logger::new("http-gateway", &sink).with_context("abcd", "ef01");
    logger.info("served request", &[("route", "query"), ("code", "200")]);
    logger.error("denied", &[("reason", "above-clearance")]);

    let records = sink.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].level, Level::Info);
    assert_eq!(records[0].trace_id.as_deref(), Some("abcd"));
    let json = records[0].to_json();
    assert!(json.contains("\"level\":\"info\""));
    assert!(json.contains("\"trace_id\":\"abcd\""));
    assert!(json.contains("\"route\":\"query\""));
}

#[test]
fn trace_context_propagates_via_w3c_traceparent() {
    let gen = IdGenerator::new(7);
    let root = gen.new_trace();
    let header = root.traceparent();
    assert!(header.starts_with("00-"));
    // A downstream service parses the header and continues the SAME trace.
    let parsed = SpanContext::parse_traceparent(&header).expect("valid traceparent");
    assert_eq!(parsed.trace_id, root.trace_id);
    let child = gen.child(&parsed);
    assert_eq!(child.trace_id, root.trace_id);
    assert_ne!(child.span_id, root.span_id);

    // Malformed headers fail closed.
    assert!(SpanContext::parse_traceparent("garbage").is_none());
    assert!(
        SpanContext::parse_traceparent("00-00000000000000000000000000000000-ef01-01").is_none()
    );

    // Spans export to an in-enclosure buffer.
    let exp = BufferExporter::new();
    exp.export(&SpanRecord {
        context: child,
        parent: Some(root.span_id),
        name: "authorize".into(),
        start: 100,
        end: 250,
        attributes: vec![("decision".into(), "permit".into())],
    });
    assert_eq!(exp.len(), 1);
    assert_eq!(exp.spans()[0].duration(), 150);
}
