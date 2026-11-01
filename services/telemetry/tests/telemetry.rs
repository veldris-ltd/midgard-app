//! Unit coverage for the telemetry pillars: metrics rendering, health probes,
//! SLO breach detection, structured logging, and trace propagation. [RC-33]

use telemetry::{
    AvailabilitySlo, BufferExporter, BufferSink, HealthRegistry, IdGenerator, LatencySlo, Level,
    Logger, MetricsRegistry, SpanContext, SpanExporter, SpanRecord, Toggle, DROPPED_SERIES,
    MAX_SERIES_PER_FAMILY,
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

// ---- WP-33: bounded cardinality ------------------------------------------------

#[test]
fn a_family_stops_growing_at_the_series_cap() {
    // [VLD-ANB-MIDGARD-0003] [AC-33.3]
    let m = MetricsRegistry::new();
    for i in 0..1_500 {
        m.inc("flood", &[("path", &format!("/probe/{i}"))]);
    }
    assert_eq!(m.series_count("flood"), MAX_SERIES_PER_FAMILY);
    assert_eq!(m.counter_value(DROPPED_SERIES, &[("family", "flood")]), 500);
    assert_eq!(m.counter_value("flood", &[("path", "/probe/1499")]), 0);

    // A series the family already holds still counts.
    m.inc("flood", &[("path", "/probe/0")]);
    assert_eq!(m.counter_value("flood", &[("path", "/probe/0")]), 2);
    assert_eq!(m.series_count("flood"), MAX_SERIES_PER_FAMILY);
    assert!(m
        .render()
        .contains("metrics_series_dropped_total{family=\"flood\"} 500"));
}

#[test]
fn histograms_are_capped_too() {
    // [VLD-ANB-MIDGARD-0003] [AC-33.3]
    let m = MetricsRegistry::new();
    for i in 0..1_500 {
        m.observe("flood_ms", &[("path", &format!("/probe/{i}"))], 3.0);
    }
    assert_eq!(m.series_count("flood_ms"), MAX_SERIES_PER_FAMILY);
    assert_eq!(
        m.counter_value(DROPPED_SERIES, &[("family", "flood_ms")]),
        500
    );
    m.observe("flood_ms", &[("path", "/probe/0")], 3.0);
    assert!(m.render().contains("flood_ms_count{path=\"/probe/0\"} 2"));

    // Gauges are held to the same cap.
    for i in 0..1_200 {
        m.set_gauge("flood_gauge", &[("id", &i.to_string())], 1.0);
    }
    assert_eq!(m.series_count("flood_gauge"), MAX_SERIES_PER_FAMILY);
    assert_eq!(
        m.counter_value(DROPPED_SERIES, &[("family", "flood_gauge")]),
        200
    );
}

#[test]
fn the_dropped_counter_is_never_capped() {
    // [VLD-ANB-MIDGARD-0003] [AC-33.3]
    let m = MetricsRegistry::new();
    // The dropped counter holds one series per family that overflowed, so it is exempt
    // from the cap: past the cap's worth of families, every one is still counted.
    let families = MAX_SERIES_PER_FAMILY + 500;
    for f in 0..families {
        m.inc(DROPPED_SERIES, &[("family", &format!("family_{f}"))]);
    }
    assert_eq!(m.series_count(DROPPED_SERIES), families);
    assert_eq!(
        m.counter_value(
            DROPPED_SERIES,
            &[("family", &format!("family_{}", families - 1))]
        ),
        1
    );

    // And a real overflow on top of that is still counted.
    for i in 0..=MAX_SERIES_PER_FAMILY {
        m.inc("flood", &[("i", &i.to_string())]);
    }
    assert_eq!(m.series_count(DROPPED_SERIES), families + 1);
    assert_eq!(m.counter_value(DROPPED_SERIES, &[("family", "flood")]), 1);
}

#[test]
fn long_label_values_are_truncated_in_the_rendering() {
    // [VLD-ANB-MIDGARD-0003]
    let m = MetricsRegistry::new();
    let long = "a".repeat(20_000);
    m.inc("long", &[("path", &long)]);
    let text = m.render();
    let expected = format!("long{{path=\"{}...\"}} 1\n", "a".repeat(256));
    assert!(text.contains(&expected), "{}", &text[..text.len().min(600)]);
    assert!(!text.contains(&"a".repeat(257)));

    // A value at the limit is left alone.
    m.inc("short", &[("path", &"b".repeat(256))]);
    assert!(m
        .render()
        .contains(&format!("short{{path=\"{}\"}} 1\n", "b".repeat(256))));
}

/// Every control character in a log field is escaped: CR by name, ESC and NUL as
/// `\u00XX`, U+2028 as `\u2028`, so no raw control byte reaches the line and the line
/// parses back to the original value. [RC-33] [AC-27.1] [VLD-ANB-MIDGARD-0033]
#[test]
fn log_lines_escape_every_control_character() {
    let value = format!("a{}b{}c{}d{}e", '\r', '\u{1b}', '\u{2028}', '\u{0}');
    let sink = BufferSink::new();
    let logger = Logger::new("http-gateway", &sink);
    logger.warn(
        &value,
        &[(value.as_str(), value.as_str()), ("path", "/x\u{7f}\t")],
    );

    let line = sink.records()[0].to_json();
    for escaped in [r"\r", r"\u001b", r"\u2028", r"\u0000", r"\u007f", r"\t"] {
        assert!(line.contains(escaped), "{escaped} missing from {line}");
    }
    assert!(
        !line
            .chars()
            .any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}'),
        "raw control character in {line:?}"
    );
    let parsed = service_runtime::json::parse(&line).expect("the line is valid JSON");
    assert_eq!(parsed.str_field("message"), Some(value.as_str()));
    assert_eq!(parsed.str_field(&value), Some(value.as_str()));
    assert_eq!(parsed.str_field("path"), Some("/x\u{7f}\t"));
}

/// A label value carrying CR and ESC renders with no raw control byte, and every line
/// of the exposition is still a comment or one `name{labels} value` sample.
/// [RC-33] [AC-27.2] [VLD-ANB-MIDGARD-0033]
#[test]
fn metric_labels_never_contain_raw_controls() {
    let m = MetricsRegistry::new();
    m.describe("http_requests_total", "total HTTP requests");
    m.inc(
        "http_requests_total",
        &[("route", "/a\r\nforged_total 99\u{1b}[0m"), ("code", "200")],
    );
    m.inc(
        "http_requests_total",
        &[("route", "/b\"\\"), ("code", "200")],
    );

    let text = m.render();
    assert!(
        !text.chars().any(|c| c.is_control() && c != '\n'),
        "raw control character in {text:?}"
    );
    assert!(
        !text.lines().any(|l| l.starts_with("forged_total")),
        "{text}"
    );
    for line in text.lines().filter(|l| !l.is_empty()) {
        if line.starts_with('#') {
            continue;
        }
        let (series, value) = line.rsplit_once(' ').expect("a sample has a value");
        assert!(value.parse::<f64>().is_ok(), "{line}");
        assert!(series.starts_with("http_requests_total{"), "{line}");
        assert!(series.ends_with('}'), "{line}");
    }
    assert!(text.contains("route=\"/b\\\"\\\\\""), "{text}");
}
