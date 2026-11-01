//! The bounded accept loop every listener in the workspace runs, and the PQ-mTLS
//! service loop built on it. [NFR-SEC-1] [RC-33] [MG-SEC-13] [VLD-ANB-MIDGARD-0004]
//!
//! [`BoundedListener`] takes a slot under a [`ConnectionGate`] before it spends a thread
//! on a connection, gives every accepted socket a read and a write timeout before
//! anything reads it, and serves it on a named thread with a stated stack. A connection
//! over the cap is handed to an `on_full` callback on the accepting thread and never
//! gets a thread; a thread that cannot be created is logged and its connection dropped;
//! a failed accept is logged without ending the service. Every outcome is counted as
//! `{name}_connections_total{outcome}`.
//!
//! It is the one connection cap: the mesh services ([`serve_mesh`]) and the HTTP edge
//! both serve through it, and later limits build on it rather than adding another.
//! [`MeshLimits`] and [`apply_mesh_limits`] turn on the gateway's per-identity rate
//! limit and per-connection request quota, which every service binary applies.

use std::fmt;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use gateway::{Gateway, RateLimiter, DEFAULT_HANDSHAKE_TIMEOUT};
use telemetry::{Logger, MetricsRegistry, StdoutJsonSink};

use crate::Args;

pub use gateway::{ConnectionGate, ConnectionPermit};

/// The stack each connection thread is given, stated rather than left to the platform
/// default. The request parser's nesting bound (`json::MAX_DEPTH`) is sized against
/// it: a body nested to the bound parses in a small fraction of this. [NFR-SEC-1]
/// [VLD-ANB-MIDGARD-0006]
pub const CONNECTION_STACK: usize = 2 * 1024 * 1024;

/// How long the accepting thread spends writing a refusal before it gives up on it.
const REFUSAL_WRITE_TIMEOUT: Duration = Duration::from_secs(1);

/// The bounds one listener serves within.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerLimits {
    /// The most connections served at once; the next is refused without a thread.
    pub max_connections: usize,
    /// Set on every accepted socket before the first read: a read that waits longer
    /// fails, and the connection is closed.
    pub read_timeout: Duration,
    /// Set on every accepted socket before the first write.
    pub write_timeout: Duration,
    /// How long a client has to send its whole request (the edge) or finish the
    /// handshake (the mesh), however it spaces its bytes.
    pub head_deadline: Duration,
}

impl ListenerLimits {
    /// The HTTP edge's defaults: 256 connections, 30 s timeouts and head deadline.
    pub const fn edge() -> Self {
        Self {
            max_connections: 256,
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            head_deadline: Duration::from_secs(30),
        }
    }

    /// A mesh service's defaults: 128 connections, 30 s timeouts and head deadline.
    /// Mesh clients open one connection per call, so an authenticated connection idle
    /// for the read timeout is closed.
    pub const fn mesh() -> Self {
        Self {
            max_connections: 128,
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            head_deadline: Duration::from_secs(30),
        }
    }
}

/// What a listener does with a connection it has admitted: serve it, holding the slot
/// until it is done.
pub type OnConnection = Arc<dyn Fn(TcpStream, ConnectionPermit) + Send + Sync>;
/// What a listener does with a connection it will not serve. Runs on the accepting
/// thread, so it must return quickly.
pub type OnRefused = Arc<dyn Fn(TcpStream) + Send + Sync>;
/// A check run on the accepting thread before a connection takes a slot. `false`
/// means the screen has answered the connection itself, and it is dropped.
pub type Screen = Arc<dyn Fn(&TcpStream, SocketAddr) -> bool + Send + Sync>;

/// An accept loop with a connection cap, socket timeouts and per-outcome metrics.
pub struct BoundedListener {
    name: String,
    log_as: String,
    limits: ListenerLimits,
    gate: ConnectionGate,
    metrics: Arc<MetricsRegistry>,
    screen: Option<Screen>,
}

impl BoundedListener {
    /// A listener named `name` — its threads are `{name}-conn` and its counter
    /// `{name}_connections_total` in `metrics` — with a gate of its own sized by
    /// `limits`.
    pub fn new(name: &str, limits: ListenerLimits, metrics: Arc<MetricsRegistry>) -> Self {
        Self {
            name: name.to_string(),
            log_as: name.to_string(),
            limits,
            gate: ConnectionGate::new(limits.max_connections),
            metrics,
            screen: None,
        }
    }

    /// Count connections against `gate` instead of a gate of its own — the mesh
    /// gateway's, so the cap is the one [`Gateway::admit`] enforces.
    pub fn with_gate(mut self, gate: ConnectionGate) -> Self {
        self.gate = gate;
        self
    }

    /// Log as `service` rather than as the listener's name.
    pub fn logging_as(mut self, service: &str) -> Self {
        self.log_as = service.to_string();
        self
    }

    /// Run `screen` on each connection before it takes a slot.
    pub fn with_screen(mut self, screen: Screen) -> Self {
        self.screen = Some(screen);
        self
    }

    /// The gate this listener counts against; clones share its count.
    pub fn gate(&self) -> ConnectionGate {
        self.gate.clone()
    }

    /// Serve `listener` for as long as the process runs.
    pub fn serve(
        self,
        listener: TcpListener,
        on_connection: OnConnection,
        on_full: OnRefused,
    ) -> ! {
        let counter = format!("{}_connections_total", self.name);
        self.metrics.describe(
            &counter,
            "Connections accepted, by whether they were served, refused, over the cap, \
             or could not be given a thread.",
        );
        loop {
            match listener.accept() {
                Ok((stream, peer)) => self.accept(&counter, stream, peer, &on_connection, &on_full),
                Err(e) => {
                    self.logger()
                        .error("accept failed", &[("reason", &e.to_string())]);
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    fn accept(
        &self,
        counter: &str,
        stream: TcpStream,
        peer: SocketAddr,
        on_connection: &OnConnection,
        on_full: &OnRefused,
    ) {
        if let Some(screen) = &self.screen {
            if !screen(&stream, peer) {
                return;
            }
        }
        let Some(permit) = self.gate.admit() else {
            self.metrics.inc(counter, &[("outcome", "over_capacity")]);
            self.logger().warn(
                "connection refused",
                &[("addr", &peer.to_string()), ("reason", "connection limit")],
            );
            let _ = stream.set_write_timeout(Some(REFUSAL_WRITE_TIMEOUT));
            on_full(stream);
            return;
        };
        // Nothing reads the socket before it has its timeouts. [VLD-ANB-MIDGARD-0004]
        if let Err(e) = stream
            .set_read_timeout(Some(self.limits.read_timeout))
            .and_then(|()| stream.set_write_timeout(Some(self.limits.write_timeout)))
        {
            self.metrics.inc(counter, &[("outcome", "refused")]);
            self.logger().warn(
                "connection refused",
                &[("addr", &peer.to_string()), ("reason", &e.to_string())],
            );
            return;
        }
        let on_connection = on_connection.clone();
        // The permit moves into the thread and is dropped when the handler returns or
        // panics; if the thread is never created it is dropped with the closure.
        let spawned = thread::Builder::new()
            .name(format!("{}-conn", self.name))
            .stack_size(CONNECTION_STACK)
            .spawn(move || on_connection(stream, permit));
        if let Err(e) = spawned {
            self.metrics.inc(counter, &[("outcome", "spawn_failed")]);
            self.logger().error(
                "cannot spawn a connection thread",
                &[("addr", &peer.to_string()), ("reason", &e.to_string())],
            );
        }
    }

    fn logger(&self) -> Logger<'static> {
        Logger::new(&self.log_as, &StdoutJsonSink)
    }
}

/// Serve `listener` within `limits` for as long as the process runs: `on_connection`
/// for each admitted connection on its own thread, `on_full` on the accepting thread
/// for each one over the cap.
pub fn serve_bounded(
    name: &str,
    listener: TcpListener,
    limits: ListenerLimits,
    metrics: Arc<MetricsRegistry>,
    on_connection: Arc<dyn Fn(TcpStream) + Send + Sync>,
    on_full: OnRefused,
) -> ! {
    BoundedListener::new(name, limits, metrics).serve(
        listener,
        Arc::new(move |stream, _permit| on_connection(stream)),
        on_full,
    )
}

/// Serve `gateway` on `listener` within [`ListenerLimits::mesh`] for as long as the
/// process runs.
pub fn serve_mesh(
    service: &str,
    listener: TcpListener,
    gateway: Gateway,
    metrics: Arc<MetricsRegistry>,
) -> ! {
    serve_mesh_with(service, listener, gateway, metrics, ListenerLimits::mesh())
}

/// [`serve_mesh`] within `limits`. The gateway's own cap becomes `limits`'
/// `max_connections` and the listener counts against it, so there is one cap. A peer
/// has the shortest of the head deadline, the read timeout and the gateway's handshake
/// timeout to finish the handshake, and an authenticated connection idle for the read
/// timeout is closed. A connection over the cap is closed at once (the client reads
/// end of stream). Outcomes are counted as `mesh_connections_total{outcome}`.
pub fn serve_mesh_with(
    service: &str,
    listener: TcpListener,
    gateway: Gateway,
    metrics: Arc<MetricsRegistry>,
    limits: ListenerLimits,
) -> ! {
    let gateway = gateway
        .with_max_connections(limits.max_connections)
        .with_handshake_timeout(
            limits
                .head_deadline
                .min(limits.read_timeout)
                .min(DEFAULT_HANDSHAKE_TIMEOUT),
        )
        .with_idle_timeout(limits.read_timeout);
    let listen = BoundedListener::new("mesh", limits, metrics.clone())
        .with_gate(gateway.connection_gate())
        .logging_as(service);
    let service = service.to_string();
    let on_connection: OnConnection = Arc::new(move |stream, permit| {
        let logger = Logger::new(&service, &StdoutJsonSink);
        let addr = stream
            .peer_addr()
            .map_or_else(|_| "unknown".to_string(), |a| a.to_string());
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
    listen.serve(listener, on_connection, Arc::new(drop))
}

/// What a mesh service serves within: its listener's bounds, and the gateway's
/// per-identity rate limit and per-connection request quota. Every service binary and
/// the gateway's mesh door apply these. [VLD-ANB-MIDGARD-0004] [RT-25]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshLimits {
    /// The listener's connection cap and timeouts.
    pub listener: ListenerLimits,
    /// Requests one identity may make in a burst. With `rate_per_sec`, both 0.0 mean
    /// no rate limit.
    pub rate_burst: f64,
    /// Requests per second one identity's allowance refills at; 0.0 with a non-zero
    /// burst makes the burst a hard quota.
    pub rate_per_sec: f64,
    /// Requests served on one connection before it is closed (0 = unlimited).
    pub requests_per_conn: usize,
}

impl Default for MeshLimits {
    /// [`ListenerLimits::mesh`], a burst of 100 and 20 a second per identity, and
    /// 10,000 requests per connection.
    fn default() -> Self {
        Self {
            listener: ListenerLimits::mesh(),
            rate_burst: 100.0,
            rate_per_sec: 20.0,
            requests_per_conn: 10_000,
        }
    }
}

impl fmt::Display for MeshLimits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "max-connections {}, idle-timeout {}s, rate-burst {}, rate-per-sec {}, \
             requests-per-conn {}",
            self.listener.max_connections,
            self.listener.read_timeout.as_secs(),
            self.rate_burst,
            self.rate_per_sec,
            self.requests_per_conn
        )
    }
}

/// `gateway` with the rate limit and request quota in `limits` turned on. A burst and
/// a rate of 0.0 together leave it without a rate limit. [VLD-ANB-MIDGARD-0004]
pub fn apply_mesh_limits(gateway: Gateway, limits: &MeshLimits) -> Gateway {
    let gateway = if limits.rate_burst == 0.0 && limits.rate_per_sec == 0.0 {
        gateway
    } else {
        gateway.with_rate_limiter(RateLimiter::new(limits.rate_burst, limits.rate_per_sec))
    };
    gateway.with_request_quota(limits.requests_per_conn)
}

/// [`MeshLimits::default`] with what the optional `--max-connections`,
/// `--idle-timeout` (seconds), `--rate-burst`, `--rate-per-sec` and
/// `--requests-per-conn` say instead. A value that is not a number, or is out of
/// range, is an error for the binary to report as a usage error.
pub fn mesh_limits_from_args(args: &Args) -> Result<MeshLimits, String> {
    mesh_limits_from_args_over(args, MeshLimits::default())
}

/// [`mesh_limits_from_args`] over a service's own defaults `base` rather than
/// [`MeshLimits::default`]: still the one limiter and the same flags.
/// [VLD-ANB-MIDGARD-0042]
pub fn mesh_limits_from_args_over(args: &Args, base: MeshLimits) -> Result<MeshLimits, String> {
    let mut limits = base;
    if let Some(n) = whole(args, "max-connections")? {
        if n == 0 {
            return Err("--max-connections must be at least 1".into());
        }
        limits.listener.max_connections = usize::try_from(n).unwrap_or(usize::MAX);
    }
    if let Some(secs) = whole(args, "idle-timeout")? {
        if secs == 0 {
            return Err("--idle-timeout must be at least 1 second".into());
        }
        limits.listener.read_timeout = Duration::from_secs(secs);
        limits.listener.write_timeout = Duration::from_secs(secs);
    }
    if let Some(burst) = rate(args, "rate-burst")? {
        limits.rate_burst = burst;
    }
    if let Some(per_sec) = rate(args, "rate-per-sec")? {
        limits.rate_per_sec = per_sec;
    }
    if limits.rate_burst < 1.0 && limits.rate_per_sec > 0.0 {
        return Err("--rate-burst must be at least 1 while --rate-per-sec is not 0".into());
    }
    if let Some(n) = whole(args, "requests-per-conn")? {
        limits.requests_per_conn = usize::try_from(n).unwrap_or(usize::MAX);
    }
    Ok(limits)
}

fn whole(args: &Args, key: &str) -> Result<Option<u64>, String> {
    args.get(key)
        .map(|v| {
            v.parse::<u64>()
                .map_err(|_| format!("--{key} must be a whole number, not '{v}'"))
        })
        .transpose()
}

fn rate(args: &Args, key: &str) -> Result<Option<f64>, String> {
    args.get(key)
        .map(|v| match v.parse::<f64>() {
            Ok(n) if n.is_finite() && n >= 0.0 => Ok(n),
            _ => Err(format!("--{key} must be a number of at least 0, not '{v}'")),
        })
        .transpose()
}
