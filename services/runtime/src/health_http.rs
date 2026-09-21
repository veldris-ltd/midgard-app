//! A plain-HTTP port for liveness, readiness and metrics beside a service's PQ-mTLS
//! port. Orchestrators and in-enclosure scrapers probe it; it serves nothing else and
//! reads nothing but a request line. [RC-33] [NFR-PRV-1]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use telemetry::{HealthRegistry, MetricsRegistry};

/// The most of a request this port reads before answering.
const MAX_HEAD: usize = 8 * 1024;

/// Bind `addr` and answer `GET /healthz` (liveness), `GET /readyz` (200 when every
/// readiness check passes, else 503) and `GET /metrics` (Prometheus text) on a
/// background thread. Returns the bound address.
pub fn spawn_health_port(
    addr: &str,
    health: Arc<HealthRegistry>,
    metrics: Arc<MetricsRegistry>,
) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    thread::spawn(move || loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let (health, metrics) = (health.clone(), metrics.clone());
                thread::spawn(move || {
                    let _ = answer(stream, &health, &metrics);
                });
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    });
    Ok(local)
}

fn answer(
    mut stream: TcpStream,
    health: &HealthRegistry,
    metrics: &MetricsRegistry,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut head = Vec::new();
    let mut buf = [0u8; 512];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < MAX_HEAD {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        head.extend_from_slice(&buf[..n]);
    }
    let text = String::from_utf8_lossy(&head);
    let mut request_line = text.split_whitespace();
    let method = request_line.next().unwrap_or_default();
    let path = request_line.next().unwrap_or_default();

    let (status, content_type, body) = match (method, path) {
        ("GET", "/healthz") => (200, "application/json", health.liveness().to_json()),
        ("GET", "/readyz") => {
            let report = health.readiness();
            let status = if report.ready { 200 } else { 503 };
            (status, "application/json", report.to_json())
        }
        ("GET", "/metrics") => (200, "text/plain; version=0.0.4", metrics.render()),
        _ => (
            404,
            "application/json",
            "{\"error\":\"not found\"}".to_string(),
        ),
    };
    let reason = match status {
        200 => "OK",
        503 => "Service Unavailable",
        _ => "Not Found",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
}
