//! # gateway — public ingress & PQ-mTLS termination
//!
//! The gateway is the only ingress into the enclosure. It terminates the
//! post-quantum mutual-TLS handshake (hybrid X25519+ML-KEM-768, fail-closed if PQ
//! is unavailable), asserts the peer's authenticated identity, authorises the
//! connection on identity/posture/purpose, and forwards requests to a handler.
//! [NFR-PQ-1] [NFR-SEC-1]
//!
//! Transport is lean std: a thread-per-connection `TcpListener`. No async runtime
//! and no external HTTP framework — the inter-service data plane is our own
//! PQ-mTLS protocol, not HTTP.
//!
//! ## Session lifecycle (RT-25)
//! One established session serves *many* request/response records (connection
//! reuse), so the handshake is paid once per connection, not per request. A
//! [`Router`] dispatches each request's method to the right control-plane handler
//! (fabric / governance / KMS). A [`RateLimiter`] enforces per-identity token
//! buckets and a per-connection request quota, so an abusive peer is throttled.
//! With a [`TicketKey`] configured, the gateway also issues PQ-mTLS **resumption
//! tickets** and accepts abbreviated (resumed) handshakes that skip the KEM +
//! signature flights — amortising the handshake across connections too.
//!
//! ## Pre-authentication limits (MG-SEC-13)
//! Everything before the handshake completes is reachable by anyone who can open a
//! socket, so it is bounded: a [connection cap](Gateway::with_max_connections) refuses
//! connections beyond it before a thread is spent on them, a
//! [handshake timeout](Gateway::with_handshake_timeout) drops a peer that stalls
//! mid-handshake, and handshake frames are capped at
//! [`pq_mtls::MAX_HANDSHAKE_FRAME`] with no memory reserved from a peer's length
//! prefix. An authenticated connection that goes quiet is dropped after the
//! [idle timeout](Gateway::with_idle_timeout).
//!
//! ## End of stream (MG-SEC-12)
//! A client ends its connection with the record layer's end-of-stream marker. A
//! connection that simply stops is reported as [`MtlsError::StreamTruncated`], so a
//! cut connection is never mistaken for a finished one.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use policy::{PolicyDecisionPoint, Resource};
use pq_mtls::{
    issue_ticket, read_frame, run_responder, run_resume_responder, write_frame, IdentityCert,
    MtlsError, Record, Responder, Role, TcpChannel, TicketKey, TransportError, TrustAnchor,
    WorkloadIdentity, MODE_FULL, MODE_RESUME,
};
use pqc_agility::{CryptoProvider, OsRng, SigningKey};

/// AAD binding every application record to this gateway's record context. A
/// client must seal/open its records under the same value.
pub const RECORD_AAD: &[u8] = b"midgard-gw/1";

/// Default cap on concurrent connections. [MG-SEC-13]
pub const DEFAULT_MAX_CONNECTIONS: usize = 512;
/// Default time a peer has to complete the handshake. [MG-SEC-13]
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Default time an authenticated connection may sit idle. [MG-SEC-13]
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Seal and send the end-of-stream marker on a record stream: how a client finishes a
/// connection so the server can tell it from a cut one. [MG-SEC-12]
pub fn close_stream(
    stream: &mut TcpStream,
    sealer: &mut pq_mtls::RecordSealer,
) -> Result<(), MtlsError> {
    write_frame(stream, &sealer.seal_close(RECORD_AAD)?).map_err(MtlsError::Transport)
}

// ---- request handling & routing -------------------------------------------

/// Handles an authenticated request. The gateway has already established the
/// peer's identity and authorised the connection before this is called.
pub trait RequestHandler: Send + Sync {
    /// Produce a response for `request` from the authenticated `peer`.
    fn handle(&self, peer: &WorkloadIdentity, request: &[u8]) -> Vec<u8>;
}

/// Any `Fn(peer, request) -> response` is a handler — convenient for wiring
/// control-plane services (fabric / governance / KMS) as closures.
impl<F> RequestHandler for F
where
    F: Fn(&WorkloadIdentity, &[u8]) -> Vec<u8> + Send + Sync,
{
    fn handle(&self, peer: &WorkloadIdentity, request: &[u8]) -> Vec<u8> {
        self(peer, request)
    }
}

/// A trivial echo handler (useful as a health/loopback endpoint and in tests).
pub struct EchoHandler;
impl RequestHandler for EchoHandler {
    fn handle(&self, _peer: &WorkloadIdentity, request: &[u8]) -> Vec<u8> {
        request.to_vec()
    }
}

/// Response status prefixes emitted by [`Router`] (first byte of the response).
pub mod status {
    /// The method was routed and the handler ran.
    pub const OK: u8 = 0;
    /// No handler is registered for the requested method.
    pub const NO_ROUTE: u8 = 1;
    /// The request envelope was malformed.
    pub const BAD_REQUEST: u8 = 2;
}

/// Encode a routed request: `u32-LE method_len ‖ method ‖ payload`.
pub fn encode_request(method: &str, payload: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(4 + method.len() + payload.len());
    o.extend_from_slice(&(method.len() as u32).to_le_bytes());
    o.extend_from_slice(method.as_bytes());
    o.extend_from_slice(payload);
    o
}

/// Decode a router response into `(status, payload)`.
pub fn decode_response(bytes: &[u8]) -> Option<(u8, &[u8])> {
    bytes.split_first().map(|(s, rest)| (*s, rest))
}

fn parse_request(request: &[u8]) -> Option<(&str, &[u8])> {
    if request.len() < 4 {
        return None;
    }
    let n = u32::from_le_bytes([request[0], request[1], request[2], request[3]]) as usize;
    let rest = &request[4..];
    if n > rest.len() {
        return None;
    }
    let method = std::str::from_utf8(&rest[..n]).ok()?;
    Some((method, &rest[n..]))
}

/// A method → handler router. Each control-plane surface (fabric, governance, KMS)
/// registers a handler; the gateway dispatches every request to the one named in
/// its envelope. Unknown methods are rejected (never silently served elsewhere).
#[derive(Default)]
pub struct Router {
    routes: HashMap<String, Arc<dyn RequestHandler>>,
}

impl Router {
    /// An empty router.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `handler` for `method` (builder style).
    pub fn route(mut self, method: &str, handler: Arc<dyn RequestHandler>) -> Self {
        self.routes.insert(method.to_string(), handler);
        self
    }

    /// The registered method names (for diagnostics/tests).
    pub fn methods(&self) -> Vec<&str> {
        self.routes.keys().map(String::as_str).collect()
    }
}

impl RequestHandler for Router {
    fn handle(&self, peer: &WorkloadIdentity, request: &[u8]) -> Vec<u8> {
        let (method, payload) = match parse_request(request) {
            Some(x) => x,
            None => return vec![status::BAD_REQUEST],
        };
        match self.routes.get(method) {
            Some(h) => {
                let mut out = Vec::with_capacity(1 + payload.len());
                out.push(status::OK);
                out.extend_from_slice(&h.handle(peer, payload));
                out
            }
            None => vec![status::NO_ROUTE],
        }
    }
}

// ---- rate limiting & quotas -----------------------------------------------

struct Bucket {
    tokens: f64,
    last: Instant,
}

/// A per-identity token-bucket rate limiter, shared across connections (so a peer
/// cannot escape its limit by reconnecting). `capacity` is the burst size;
/// `refill_per_sec` the sustained rate (0.0 makes it a pure hard quota of
/// `capacity` requests). [RT-25]
pub struct RateLimiter {
    capacity: f64,
    refill_per_sec: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    /// Build a limiter with the given burst `capacity` and `refill_per_sec` rate.
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            capacity,
            refill_per_sec,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Consume one token for `id`. Returns `false` (throttle) if the bucket is dry.
    pub fn allow(&self, id: &str) -> bool {
        let now = Instant::now();
        let mut m = self.buckets.lock().expect("rate-limiter lock");
        let b = m.entry(id.to_string()).or_insert(Bucket {
            tokens: self.capacity,
            last: now,
        });
        if self.refill_per_sec > 0.0 {
            let dt = now.duration_since(b.last).as_secs_f64();
            b.tokens = (b.tokens + dt * self.refill_per_sec).min(self.capacity);
            b.last = now;
        }
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

// ---- the gateway ----------------------------------------------------------

type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

fn system_clock() -> Clock {
    Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    })
}

/// The gateway's static configuration, cheaply cloneable (Arc-shared) so each
/// accepted connection can be handled on its own thread.
#[derive(Clone)]
pub struct Gateway {
    provider: Arc<CryptoProvider>,
    cert: IdentityCert,
    signing_key: Arc<SigningKey>,
    trust_anchor: TrustAnchor,
    pdp: Arc<dyn PolicyDecisionPoint + Send + Sync>,
    resource: Resource,
    handler: Arc<dyn RequestHandler>,
    rate_limiter: Option<Arc<RateLimiter>>,
    max_requests_per_conn: usize,
    ticket: Option<(Arc<TicketKey>, u64)>, // (key, ttl secs)
    seen_tickets: Arc<Mutex<HashSet<Vec<u8>>>>,
    clock: Clock,
    max_connections: usize,
    active: Arc<AtomicUsize>,
    handshake_timeout: Duration,
    idle_timeout: Duration,
}

/// A slot under the gateway's connection cap, released when dropped. [MG-SEC-13]
pub struct ConnectionPermit {
    active: Arc<AtomicUsize>,
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Gateway {
    /// Assemble a gateway.
    ///
    /// - `provider`: the crypto provider (must offer the hybrid PQ suites).
    /// - `cert` / `signing_key`: the gateway's own IdP-issued workload identity.
    /// - `trust_anchor`: the IdP CA the gateway authenticates peers against.
    /// - `pdp` / `resource`: the authorization policy for opening a channel.
    /// - `handler`: what to do with an authenticated request (often a [`Router`]).
    ///
    /// Rate limiting, request quotas, and resumption are opt-in via the builder
    /// methods; a bare `new` gateway keeps the original single-surface behaviour.
    pub fn new(
        provider: Arc<CryptoProvider>,
        cert: IdentityCert,
        signing_key: Arc<SigningKey>,
        trust_anchor: TrustAnchor,
        pdp: Arc<dyn PolicyDecisionPoint + Send + Sync>,
        resource: Resource,
        handler: Arc<dyn RequestHandler>,
    ) -> Self {
        Self {
            provider,
            cert,
            signing_key,
            trust_anchor,
            pdp,
            resource,
            handler,
            rate_limiter: None,
            max_requests_per_conn: 0,
            ticket: None,
            seen_tickets: Arc::new(Mutex::new(HashSet::new())),
            clock: system_clock(),
            max_connections: DEFAULT_MAX_CONNECTIONS,
            active: Arc::new(AtomicUsize::new(0)),
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    /// Serve at most `max` connections at once; further ones are refused with
    /// [`MtlsError::Overloaded`] before any handshake work. [MG-SEC-13]
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// Drop a peer that has not completed the handshake within `timeout`. [MG-SEC-13]
    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// Drop an authenticated connection that sends nothing for `timeout`. [MG-SEC-13]
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Take a slot under the connection cap, or `None` if the gateway is full. Accept
    /// loops call this before spending a thread on a connection. [MG-SEC-13]
    pub fn admit(&self) -> Option<ConnectionPermit> {
        let mut current = self.active.load(Ordering::Acquire);
        loop {
            if current >= self.max_connections {
                return None;
            }
            match self.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(ConnectionPermit {
                        active: self.active.clone(),
                    })
                }
                Err(seen) => current = seen,
            }
        }
    }

    /// Connections currently being served.
    pub fn active_connections(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    /// Enforce a per-identity token-bucket rate limit.
    pub fn with_rate_limiter(mut self, limiter: RateLimiter) -> Self {
        self.rate_limiter = Some(Arc::new(limiter));
        self
    }

    /// Cap the number of requests served on any single connection (0 = unlimited).
    pub fn with_request_quota(mut self, max_requests_per_conn: usize) -> Self {
        self.max_requests_per_conn = max_requests_per_conn;
        self
    }

    /// Enable PQ-mTLS session resumption: issue tickets valid for `ttl_secs` and
    /// accept abbreviated handshakes that present a valid one.
    pub fn with_resumption(mut self, key: TicketKey, ttl_secs: u64) -> Self {
        self.ticket = Some((Arc::new(key), ttl_secs));
        self
    }

    /// Override the wall clock (seconds) used for ticket issuance/expiry — for
    /// deterministic tests.
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// Terminate PQ-mTLS on one connection, authorise it, and service requests
    /// over the record layer until the peer closes (or is throttled). Returns the
    /// authenticated peer. Any handshake, authorization, or transport failure
    /// returns an error and the connection is dropped — there is no unauthenticated
    /// path to the handler. [AC-M-1] [AC-M-3] [RT-25]
    pub fn handle_connection(&self, stream: TcpStream) -> Result<WorkloadIdentity, MtlsError> {
        let permit = self.admit().ok_or(MtlsError::Overloaded)?;
        self.handle_admitted(permit, stream)
    }

    /// [`Gateway::handle_connection`] for a connection already holding a slot from
    /// [`Gateway::admit`]; the slot is released when this returns.
    pub fn handle_admitted(
        &self,
        permit: ConnectionPermit,
        stream: TcpStream,
    ) -> Result<WorkloadIdentity, MtlsError> {
        let _permit = permit;
        // Everything up to an established session must finish within the handshake
        // timeout: a peer that stalls is dropped, not waited on. [MG-SEC-13]
        let io = |e: std::io::Error| MtlsError::Transport(TransportError::Io(e.to_string()));
        stream
            .set_read_timeout(Some(self.handshake_timeout))
            .map_err(io)?;
        stream
            .set_write_timeout(Some(self.handshake_timeout))
            .map_err(io)?;
        if self.ticket.is_some() {
            // Resumption-capable: the client leads with a one-byte connection mode.
            let mut mode = [0u8; 1];
            stream
                .try_clone()
                .and_then(|mut s| s.read_exact(&mut mode))
                .map_err(|e| MtlsError::Transport(pq_mtls::TransportError::Io(e.to_string())))?;
            match mode[0] {
                MODE_RESUME => self.serve_resumed(stream),
                MODE_FULL => self.serve_full(stream, true),
                other => Err(MtlsError::Protocol(format!(
                    "unknown connection mode {other}"
                ))),
            }
        } else {
            // Legacy path: straight into a full handshake, no ticket issued.
            self.serve_full(stream, false)
        }
    }

    /// Full handshake, then (optionally) issue a resumption ticket, then serve.
    fn serve_full(&self, stream: TcpStream, issue: bool) -> Result<WorkloadIdentity, MtlsError> {
        let mut rng = OsRng;
        let responder = Responder::new(
            self.provider.as_ref(),
            self.cert.clone(),
            self.signing_key.as_ref(),
            self.trust_anchor.clone(),
            self.pdp.as_ref(),
            self.resource.clone(),
            &mut rng,
        )
        .at_time((self.clock)());
        let mut ch = TcpChannel::new(stream);
        let session = run_responder(responder, &mut ch)?;
        let peer = session.peer().clone();
        let mut stream = ch.into_stream();

        // Hand the client a resumption ticket before the request loop. The ticket
        // carries what this handshake authorised — the clearance capped by the
        // client's certificate and its declared purpose. [MG-SEC-04]
        if issue {
            if let Some((key, ttl)) = &self.ticket {
                let now = (self.clock)();
                let ticket = issue_ticket(key, &session, now, *ttl, &mut rng)?;
                write_frame(&mut stream, &ticket).map_err(MtlsError::Transport)?;
            }
        }

        let (sealer, opener) = session.records(Role::Responder);
        self.serve_requests(&mut stream, &peer, sealer, opener)?;
        Ok(peer)
    }

    /// Abbreviated (resumed) handshake, then serve — no KEM, no signatures.
    fn serve_resumed(&self, stream: TcpStream) -> Result<WorkloadIdentity, MtlsError> {
        let (key, _ttl) = self
            .ticket
            .as_ref()
            .ok_or_else(|| MtlsError::Protocol("resumption not configured".into()))?;
        let mut stream = stream;
        let mut rng = OsRng;
        let now = (self.clock)();
        let seen = self.seen_tickets.clone();
        let resumed = run_resume_responder(
            &mut stream,
            key,
            &self.trust_anchor,
            self.pdp.as_ref(),
            &self.resource,
            now,
            // Single-use: `insert` returns true only if the ticket is unseen.
            |t| seen.lock().expect("ticket set").insert(t.to_vec()),
            &mut rng,
        )?;
        let peer = resumed.client.clone();
        let (sealer, opener) = resumed.session.records(Role::Responder);
        self.serve_requests(&mut stream, &peer, sealer, opener)?;
        Ok(peer)
    }

    /// The record loop: serve request/response frames until the client's
    /// end-of-stream marker, a rate limit, or the per-connection quota. A
    /// throttled/over-quota peer's connection is dropped (its next read fails) — the
    /// record layer's sequence numbers stay in lock-step because we only ever seal
    /// responses in order. A connection that ends without the marker is
    /// [`MtlsError::StreamTruncated`]. [MG-SEC-12]
    fn serve_requests(
        &self,
        stream: &mut TcpStream,
        peer: &WorkloadIdentity,
        mut sealer: pq_mtls::RecordSealer,
        mut opener: pq_mtls::RecordOpener,
    ) -> Result<(), MtlsError> {
        let io = |e: std::io::Error| MtlsError::Transport(TransportError::Io(e.to_string()));
        stream
            .set_read_timeout(Some(self.idle_timeout))
            .map_err(io)?;
        stream
            .set_write_timeout(Some(self.idle_timeout))
            .map_err(io)?;
        let mut served = 0usize;
        loop {
            let frame = match read_frame(stream) {
                Ok(f) if !f.is_empty() => f,
                _ => return Err(MtlsError::StreamTruncated),
            };
            // Per-identity rate limit.
            if let Some(rl) = &self.rate_limiter {
                if !rl.allow(&peer.id.0) {
                    break; // throttled: drop the connection
                }
            }
            // Per-connection request quota.
            served += 1;
            if self.max_requests_per_conn != 0 && served > self.max_requests_per_conn {
                break;
            }
            let request = match opener.open_record(RECORD_AAD, &frame)? {
                Record::Data(request) => request,
                Record::Close => break,
            };
            let response = self.handler.handle(peer, &request);
            write_frame(stream, &sealer.seal(RECORD_AAD, &response)?)
                .map_err(MtlsError::Transport)?;
        }
        Ok(())
    }

    /// Serve connections until `listener` errors, one thread per connection, up to
    /// the connection cap: a connection over the cap is reported as
    /// [`MtlsError::Overloaded`] and closed without a thread. Per-connection failures
    /// are isolated (logged by the closure) and never take down the listener.
    pub fn serve(
        &self,
        listener: &TcpListener,
        on_result: impl Fn(Result<WorkloadIdentity, MtlsError>) + Send + Sync + 'static,
    ) where
        Self: 'static,
    {
        let on_result = Arc::new(on_result);
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => match self.admit() {
                    Some(permit) => {
                        let gw = self.clone();
                        let cb = on_result.clone();
                        std::thread::spawn(move || cb(gw.handle_admitted(permit, stream)));
                    }
                    None => {
                        drop(stream);
                        on_result(Err(MtlsError::Overloaded));
                    }
                },
                Err(_) => break,
            }
        }
    }
}
