//! The PQ-mTLS accept loop a service runs: one thread per connection up to the
//! gateway's connection cap, every outcome counted and logged, and a failed accept
//! logged without ending the service. A connection over the cap is closed at once,
//! without a thread. [NFR-SEC-1] [RC-33] [MG-SEC-13]

use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use gateway::Gateway;
use telemetry::{Logger, MetricsRegistry, StdoutJsonSink};

/// Serve `gateway` on `listener` for as long as the process runs.
pub fn serve_mesh(
    service: &str,
    listener: TcpListener,
    gateway: Gateway,
    metrics: Arc<MetricsRegistry>,
) -> ! {
    metrics.describe(
        "mesh_connections_total",
        "PQ-mTLS connections, by whether they were served, refused, or over the cap.",
    );
    loop {
        match listener.accept() {
            Ok((stream, peer)) => {
                let Some(permit) = gateway.admit() else {
                    drop(stream);
                    metrics.inc("mesh_connections_total", &[("outcome", "overloaded")]);
                    Logger::new(service, &StdoutJsonSink).warn(
                        "connection refused",
                        &[("addr", &peer.to_string()), ("reason", "connection limit")],
                    );
                    continue;
                };
                let gateway = gateway.clone();
                let metrics = metrics.clone();
                let service = service.to_string();
                thread::spawn(move || {
                    let logger = Logger::new(&service, &StdoutJsonSink);
                    let addr = peer.to_string();
                    match gateway.handle_admitted(permit, stream) {
                        Ok(identity) => {
                            metrics.inc("mesh_connections_total", &[("outcome", "served")]);
                            logger.info(
                                "connection served",
                                &[("peer", &identity.id.0), ("addr", &addr)],
                            );
                        }
                        Err(e) => {
                            metrics.inc("mesh_connections_total", &[("outcome", "refused")]);
                            logger.warn(
                                "connection refused",
                                &[("addr", &addr), ("reason", &format!("{e:?}"))],
                            );
                        }
                    }
                });
            }
            Err(e) => {
                Logger::new(service, &StdoutJsonSink)
                    .error("accept failed", &[("reason", &e.to_string())]);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}
