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
//! tickets** and accepts abbreviated (resumed) handshakes that skip the signature
//! flights — amortising the handshake across connections too.
//!
//! ## Resumption (VLD-ANB-MIDGARD-0046)
//! The ticket travels inside the session's record layer, sealed under
//! [`pq_mtls::NEW_TICKET_AAD`] as the first record the gateway sends. Each resumption
//! runs a fresh KEM exchange and re-verifies the client's certificate against the
//! gateway's live trust anchor and revocation list. Ticket keys rotate once per
//! ticket TTL; the previous key is kept so tickets issued before a rotation still
//! open while they are in date. Seen tickets are remembered as SHA-256 digests until
//! they expire, at most [`MAX_SEEN_TICKETS`] of them; when the set is full a
//! resumption is refused (the client falls back to a full handshake) and counted.
//! Resumption is opt-in ([`Gateway::with_resumption`]) and no service enables it.
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

use std::collections::HashMap;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use policy::{PolicyDecisionPoint, Resource};
use pq_mtls::{
    issue_ticket, read_frame, run_responder, run_resume_responder, write_frame, IdentityCert,
    MtlsError, Record, Responder, Role, TcpChannel, TicketKey, TransportError, TrustAnchor,
    WorkloadIdentity, MODE_FULL, MODE_RESUME, NEW_TICKET_AAD,
};
use pqc_agility::{sha256, CryptoProvider, OsRng, Rng, SigningKey};

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
///
/// A bucket that has refilled to `capacity` is the same as no bucket, so once the map
/// reaches [`RATE_LIMITER_PRUNE_AT`] identities (and each time it doubles from what a
/// prune left) such buckets are dropped: the map holds only the identities seen within
/// one refill period, not every identity ever seen. [VLD-ANB-MIDGARD-0005]
pub struct RateLimiter {
    capacity: f64,
    refill_per_sec: f64,
    buckets: Mutex<Buckets>,
}

/// How many identities a [`RateLimiter`] holds before it first drops refilled buckets.
pub const RATE_LIMITER_PRUNE_AT: usize = 1024;

struct Buckets {
    map: HashMap<String, Bucket>,
    /// The size at which the next new identity prunes the map first.
    prune_at: usize,
}

impl RateLimiter {
    /// Build a limiter with the given burst `capacity` and `refill_per_sec` rate.
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            capacity,
            refill_per_sec,
            buckets: Mutex::new(Buckets {
                map: HashMap::new(),
                prune_at: RATE_LIMITER_PRUNE_AT,
            }),
        }
    }

    /// Consume one token for `id`. Returns `false` (throttle) if the bucket is dry.
    pub fn allow(&self, id: &str) -> bool {
        let now = Instant::now();
        let mut guard = self.buckets.lock().expect("rate-limiter lock");
        let buckets = &mut *guard;
        if buckets.map.len() >= buckets.prune_at && !buckets.map.contains_key(id) {
            self.prune(&mut buckets.map, now);
            buckets.prune_at = (2 * buckets.map.len()).max(RATE_LIMITER_PRUNE_AT);
        }
        let b = buckets.map.entry(id.to_string()).or_insert(Bucket {
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

    /// How many identities the limiter holds a bucket for.
    pub fn tracked(&self) -> usize {
        self.buckets.lock().expect("rate-limiter lock").map.len()
    }

    /// Drop every bucket that has refilled to capacity by `now`. A hard quota (no
    /// refill) never refills, so it keeps every bucket.
    fn prune(&self, map: &mut HashMap<String, Bucket>, now: Instant) {
        if self.refill_per_sec <= 0.0 {
            return;
        }
        map.retain(|_, b| {
            let dt = now.duration_since(b.last).as_secs_f64();
            b.tokens + dt * self.refill_per_sec < self.capacity
        });
    }
}

// ---- resumption state ------------------------------------------------------

/// The most seen-ticket digests the gateway remembers. When the set is full (and
/// nothing in it has expired) a resumption is refused and the client falls back to a
/// full handshake. [VLD-ANB-MIDGARD-0046]
pub const MAX_SEEN_TICKETS: usize = 65_536;

/// What [`SeenTickets::admit`] decided about a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketAdmission {
    /// First presentation: the ticket is now marked as used.
    Fresh,
    /// The ticket was presented before.
    Replayed,
    /// The set is at [`MAX_SEEN_TICKETS`]; the ticket is refused unrecorded.
    Full,
}

/// The single-use guard for resumption tickets: SHA-256 digests of the tickets
/// presented so far, each kept until the ticket's own expiry. Expired entries are
/// pruned on insert, and the set never holds more than [`MAX_SEEN_TICKETS`].
/// [VLD-ANB-MIDGARD-0046]
#[derive(Debug)]
pub struct SeenTickets {
    entries: HashMap<[u8; 32], u64>,
    /// The earliest expiry in `entries` (`u64::MAX` when empty), so pruning only
    /// walks the set once something in it can have expired.
    earliest: u64,
}

impl Default for SeenTickets {
    fn default() -> Self {
        Self::new()
    }
}

impl SeenTickets {
    /// An empty set.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            earliest: u64::MAX,
        }
    }

    /// Record `ticket` (valid until `expires_at`, inclusive) as used at `now`, after
    /// pruning the entries that expired before `now`.
    pub fn admit(&mut self, ticket: &[u8], expires_at: u64, now: u64) -> TicketAdmission {
        self.prune(now);
        let digest = sha256(ticket);
        if self.entries.contains_key(&digest) {
            return TicketAdmission::Replayed;
        }
        if self.entries.len() >= MAX_SEEN_TICKETS {
            return TicketAdmission::Full;
        }
        self.entries.insert(digest, expires_at);
        self.earliest = self.earliest.min(expires_at);
        TicketAdmission::Fresh
    }

    /// How many tickets are remembered.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no ticket is remembered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn prune(&mut self, now: u64) {
        if now <= self.earliest {
            return;
        }
        self.entries.retain(|_, expires_at| *expires_at >= now);
        self.earliest = self.entries.values().copied().min().unwrap_or(u64::MAX);
    }
}

/// The gateway's ticket keys: the one new tickets are sealed under and the one before
/// it. The current key is replaced once it is older than the ticket TTL, so every
/// ticket still in date was sealed under one of the two. [VLD-ANB-MIDGARD-0046]
struct TicketKeys {
    current: Arc<TicketKey>,
    previous: Option<Arc<TicketKey>>,
    /// When `current` came into use (`None` until the first connection, so a clock
    /// injected after [`Gateway::with_resumption`] is honoured).
    rotated_at: Option<u64>,
    ttl: u64,
}

impl TicketKeys {
    /// Rotate if the current key has been in use for longer than the TTL, then return
    /// the current and previous keys.
    fn at(&mut self, now: u64, rng: &mut dyn Rng) -> (Arc<TicketKey>, Option<Arc<TicketKey>>) {
        match self.rotated_at {
            None => self.rotated_at = Some(now),
            Some(at) if now.saturating_sub(at) > self.ttl => {
                let next = TicketKey::generate(rng).with_id(self.current.id().wrapping_add(1));
                self.previous = Some(std::mem::replace(&mut self.current, Arc::new(next)));
                self.rotated_at = Some(now);
            }
            Some(_) => {}
        }
        (self.current.clone(), self.previous.clone())
    }
}

// ---- the gateway ----------------------------------------------------------

type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// Told the identity of every request the rate limit refuses, for the service's
/// metrics. [VLD-ANB-MIDGARD-0016]
pub type ThrottleHook = Arc<dyn Fn(&WorkloadIdentity) + Send + Sync>;

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
    on_throttle: Option<ThrottleHook>,
    max_requests_per_conn: usize,
    ticket: Option<Arc<Mutex<TicketKeys>>>,
    seen_tickets: Arc<Mutex<SeenTickets>>,
    replay_cache_full: Arc<AtomicU64>,
    clock: Clock,
    gate: ConnectionGate,
    handshake_timeout: Duration,
    idle_timeout: Duration,
}

/// A cap on concurrent connections: a counter of live ones, shared by every clone,
/// and the most it lets in. The one connection cap in the workspace — the mesh gateway
/// and the HTTP edge's bounded listener both count against one of these, rather than
/// each keeping its own. [MG-SEC-13] [VLD-ANB-MIDGARD-0004]
#[derive(Clone)]
pub struct ConnectionGate {
    max: usize,
    active: Arc<AtomicUsize>,
}

impl ConnectionGate {
    /// A gate letting in at most `max` connections at once.
    pub fn new(max: usize) -> Self {
        Self {
            max,
            active: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Take a slot, or `None` if the gate is full. Accept loops call this before
    /// spending a thread on a connection.
    pub fn admit(&self) -> Option<ConnectionPermit> {
        let mut current = self.active.load(Ordering::Acquire);
        loop {
            if current >= self.max {
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

    /// Connections currently holding a slot.
    pub fn live_connections(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    /// The most connections the gate lets in at once.
    pub fn max_connections(&self) -> usize {
        self.max
    }
}

/// A slot under a [`ConnectionGate`], released when dropped — so a handler that
/// panics still gives its slot back. [MG-SEC-13]
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
    /// Every service binary turns them on through `service_runtime::apply_mesh_limits`.
    /// [VLD-ANB-MIDGARD-0004]
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
            on_throttle: None,
            max_requests_per_conn: 0,
            ticket: None,
            seen_tickets: Arc::new(Mutex::new(SeenTickets::new())),
            replay_cache_full: Arc::new(AtomicU64::new(0)),
            clock: system_clock(),
            gate: ConnectionGate::new(DEFAULT_MAX_CONNECTIONS),
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    /// Serve at most `max` connections at once; further ones are refused with
    /// [`MtlsError::Overloaded`] before any handshake work. [MG-SEC-13]
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.gate = ConnectionGate::new(max);
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
        self.gate.admit()
    }

    /// Connections currently being served.
    pub fn active_connections(&self) -> usize {
        self.gate.live_connections()
    }

    /// The gate this gateway's connections count against, for an accept loop that
    /// takes slots itself before calling [`Gateway::handle_admitted`]. Clones share
    /// the count. [VLD-ANB-MIDGARD-0004]
    pub fn connection_gate(&self) -> ConnectionGate {
        self.gate.clone()
    }

    /// Enforce a per-identity token-bucket rate limit.
    pub fn with_rate_limiter(mut self, limiter: RateLimiter) -> Self {
        self.rate_limiter = Some(Arc::new(limiter));
        self
    }

    /// Call `hook` with the peer of every request the rate limit refuses, before its
    /// connection is dropped. [VLD-ANB-MIDGARD-0016]
    pub fn on_throttle(mut self, hook: ThrottleHook) -> Self {
        self.on_throttle = Some(hook);
        self
    }

    /// Cap the number of requests served on any single connection (0 = unlimited).
    pub fn with_request_quota(mut self, max_requests_per_conn: usize) -> Self {
        self.max_requests_per_conn = max_requests_per_conn;
        self
    }

    /// Enable PQ-mTLS session resumption: issue tickets valid for `ttl_secs` and
    /// accept abbreviated handshakes that present a valid one. `key` seals tickets
    /// first; the gateway replaces it with a fresh random key (the next key id) once
    /// it is older than `ttl_secs`, keeping the previous one to open tickets issued
    /// before. [VLD-ANB-MIDGARD-0046]
    pub fn with_resumption(mut self, key: TicketKey, ttl_secs: u64) -> Self {
        self.ticket = Some(Arc::new(Mutex::new(TicketKeys {
            current: Arc::new(key),
            previous: None,
            rotated_at: None,
            ttl: ttl_secs,
        })));
        self
    }

    /// How many resumptions were refused because the seen-ticket set was full (each
    /// client fell back to a full handshake). [VLD-ANB-MIDGARD-0046]
    pub fn resumptions_refused_replay_cache_full(&self) -> u64 {
        self.replay_cache_full.load(Ordering::Relaxed)
    }

    /// The current and previous ticket keys and the ticket TTL at `now`, rotating
    /// first if due.
    fn ticket_keys(
        keys: &Mutex<TicketKeys>,
        now: u64,
    ) -> (Arc<TicketKey>, Option<Arc<TicketKey>>, u64) {
        let mut keys = keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (current, previous) = keys.at(now, &mut OsRng);
        (current, previous, keys.ttl)
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

        let (mut sealer, opener) = session.records(Role::Responder);
        // Hand the client a resumption ticket before the request loop, as the first
        // sealed record, never in clear. The ticket carries the client's certificate
        // and what this handshake authorised: the clearance capped by the
        // certificate and the declared purpose. [MG-SEC-04] [VLD-ANB-MIDGARD-0046]
        if issue {
            if let Some(keys) = &self.ticket {
                let now = (self.clock)();
                let (key, _, ttl) = Self::ticket_keys(keys, now);
                let ticket = issue_ticket(&key, &session, now, ttl, &mut rng)?;
                write_frame(&mut stream, &sealer.seal(NEW_TICKET_AAD, &ticket)?)
                    .map_err(MtlsError::Transport)?;
            }
        }

        self.serve_requests(&mut stream, &peer, sealer, opener)?;
        Ok(peer)
    }

    /// Abbreviated (resumed) handshake, then serve: a fresh KEM exchange and a
    /// certificate re-check, but no signature flights. [VLD-ANB-MIDGARD-0046]
    fn serve_resumed(&self, stream: TcpStream) -> Result<WorkloadIdentity, MtlsError> {
        let keys = self
            .ticket
            .as_ref()
            .ok_or_else(|| MtlsError::Protocol("resumption not configured".into()))?;
        let mut stream = stream;
        let mut rng = OsRng;
        let now = (self.clock)();
        let (current, previous, _) = Self::ticket_keys(keys, now);
        let mut open_with: Vec<&TicketKey> = vec![&current];
        open_with.extend(previous.as_deref());
        let resumed = run_resume_responder(
            &mut stream,
            &open_with,
            // The anchor's revocation state is shared by every clone, so this is the
            // list in force now, reloaded live. [MG-SEC-03]
            &self.trust_anchor,
            self.provider.as_ref(),
            self.pdp.as_ref(),
            &self.resource,
            now,
            // Single-use, and only reached once the binder has verified.
            |t, expires_at| {
                let admission = self
                    .seen_tickets
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .admit(t, expires_at, now);
                if admission == TicketAdmission::Full {
                    self.replay_cache_full.fetch_add(1, Ordering::Relaxed);
                }
                admission == TicketAdmission::Fresh
            },
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
            // The client's end-of-stream marker is not a request: it is neither
            // rate-limited nor counted against the quota. [VLD-ANB-MIDGARD-0016]
            let request = match opener.open_record(RECORD_AAD, &frame)? {
                Record::Data(request) => request,
                Record::Close => break,
            };
            // Per-identity rate limit.
            if let Some(rl) = &self.rate_limiter {
                if !rl.allow(&peer.id.0) {
                    if let Some(hook) = &self.on_throttle {
                        hook(peer);
                    }
                    break; // throttled: drop the connection
                }
            }
            // Per-connection request quota.
            served += 1;
            if self.max_requests_per_conn != 0 && served > self.max_requests_per_conn {
                break;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_limiter_drops_refilled_buckets_rather_than_growing() {
        // [VLD-ANB-MIDGARD-0005]
        let limiter = RateLimiter::new(1.0, 1_000.0);
        for i in 0..RATE_LIMITER_PRUNE_AT {
            assert!(limiter.allow(&format!("10.0.{}.{}", i / 256, i % 256)));
        }
        assert_eq!(limiter.tracked(), RATE_LIMITER_PRUNE_AT);
        // Every bucket refills within a millisecond; the next new identity prunes them.
        std::thread::sleep(Duration::from_millis(20));
        assert!(limiter.allow("10.9.9.9"));
        assert_eq!(limiter.tracked(), 1, "only the new identity is held");

        // A hard quota never refills, so its buckets are kept and still refuse.
        let quota = RateLimiter::new(1.0, 0.0);
        for i in 0..=RATE_LIMITER_PRUNE_AT {
            assert!(quota.allow(&format!("peer-{i}")));
        }
        assert_eq!(quota.tracked(), RATE_LIMITER_PRUNE_AT + 1);
        assert!(!quota.allow("peer-0"), "a spent quota stays spent");
    }
}
