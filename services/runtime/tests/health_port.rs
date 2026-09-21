//! The health port on a real socket: live while the process runs, ready only when every
//! readiness check passes, metrics in Prometheus text, and nothing else served.
//! [RC-33] [NFR-PRV-1]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;

use service_runtime::spawn_health_port;
use telemetry::{HealthRegistry, MetricsRegistry, Toggle};

fn request(addr: SocketAddr, method: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: probe\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status line");
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

#[test]
fn health_readiness_and_metrics_are_served_and_nothing_else() {
    let ledger = Arc::new(Toggle::new("audit-ledger", false));
    let mut health = HealthRegistry::new();
    health.register(Box::new(ledger.clone()));
    let metrics = Arc::new(MetricsRegistry::new());
    metrics.describe("probes_total", "Probes.");
    metrics.inc("probes_total", &[("kind", "test")]);
    let addr = spawn_health_port("127.0.0.1:0", Arc::new(health), metrics).unwrap();

    let (status, body) = request(addr, "GET", "/healthz");
    assert_eq!(status, 200);
    assert!(body.contains("\"live\":true"), "{body}");

    let (status, body) = request(addr, "GET", "/readyz");
    assert_eq!(status, 503, "not ready until the ledger is");
    assert!(body.contains("\"name\":\"audit-ledger\""), "{body}");

    ledger.set(true);
    let (status, body) = request(addr, "GET", "/readyz");
    assert_eq!(status, 200);
    assert!(body.contains("\"ready\":true"), "{body}");

    let (status, body) = request(addr, "GET", "/metrics");
    assert_eq!(status, 200);
    assert!(body.contains("probes_total{kind=\"test\"} 1"), "{body}");

    assert_eq!(request(addr, "GET", "/v1/infer").0, 404);
    assert_eq!(request(addr, "POST", "/readyz").0, 404);
}
