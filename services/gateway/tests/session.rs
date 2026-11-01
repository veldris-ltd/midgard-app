//! RC-25: the gateway is a real session boundary, not one-request-per-connection.
//!
//! - **Connection reuse:** one PQ-mTLS handshake serves many request/response
//!   records (the load test fires 50 on a single session).
//! - **Routing:** each request's method is dispatched to the right control-plane
//!   handler — including the *real* governance spine — and an unknown method is
//!   refused (never misrouted).
//! - **Resumption:** a second connection presents the ticket issued on the first
//!   and completes an abbreviated handshake (a fresh KEM exchange, no signatures);
//!   the ticket is single-use. The ticket arrives inside the record layer, a bad
//!   binder cannot consume it, a revoked or expired client certificate cannot
//!   resume, the seen-ticket set is bounded and ticket keys rotate.
//!   [VLD-ANB-MIDGARD-0046]
//! - **Rate limiting:** an abusive peer that exceeds its token bucket is throttled
//!   (its connection is dropped). [RC-25]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use gateway::{
    decode_response, encode_request, status, Gateway, RateLimiter, RequestHandler, Router,
    SeenTickets, TicketAdmission, MAX_SEEN_TICKETS, RECORD_AAD,
};
use governance::{GovContext, Governance};
use iam::{Idp, IDP_SIG_ALG};
use policy::{
    Action, ClassificationLevel, Clearance, DefaultPdp, Effect, Marking, PolicyDecisionPoint,
    PolicySet, Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{
    read_frame, run_initiator, run_resume_initiator, write_frame, ClientTicket, IdentityCert,
    Initiator, MtlsError, Posture, Role as RecordRole, ServiceId, TcpChannel, TicketKey,
    WorkloadIdentity, MODE_FULL, MODE_RESUME, NEW_TICKET_AAD,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{CryptoProvider, KemSuiteId, Rng, SigningKey};

// ---- deterministic RNG (shared with the handshake tests) ------------------

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let b = z.to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&b[..take]);
            i += take;
        }
    }
}

fn workload(idp: &Idp, id: &str, roles: &[&str], seed: u64) -> (IdentityCert, SigningKey) {
    workload_until(idp, id, roles, seed, u64::MAX)
}

/// A workload certificate valid from the epoch until `not_after`.
fn workload_until(
    idp: &Idp,
    id: &str,
    roles: &[&str],
    seed: u64,
    not_after: u64,
) -> (IdentityCert, SigningKey) {
    let (vk, sk) = idp
        .provider()
        .sig_for(IDP_SIG_ALG)
        .unwrap()
        .keypair(&mut SeedRng(seed))
        .unwrap();
    let identity = WorkloadIdentity {
        id: ServiceId(id.into()),
        roles: roles.iter().map(|r| Role(r.to_string())).collect(),
        sig_alg: IDP_SIG_ALG,
        verifying_key: vk,
    };
    (
        idp.certify(
            &identity,
            pq_mtls::CertTerms::new(
                1,
                0,
                not_after,
                pq_mtls::Posture::at(policy::ClassificationLevel::TOP_SECRET),
            ),
        )
        .unwrap(),
        sk,
    )
}

fn channel_resource() -> Resource {
    Resource {
        id: ResourceId("channel".into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

fn permit_pdp(role: &str) -> DefaultPdp {
    DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("r".into()),
            role: Role(role.into()),
            action: Action("OpenChannel".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    )
}

fn posture() -> Posture {
    Posture {
        level: ClassificationLevel::UNCLASSIFIED,
        compartments: BTreeSet::new(),
    }
}

/// A router wiring three distinct control-plane surfaces. `governance.authorize` is
/// backed by the **real** governance crate, so a request on that route genuinely
/// reaches (and audits through) the governance spine.
fn control_plane_router() -> (Arc<dyn RequestHandler>, Arc<Mutex<Governance>>) {
    let gov = Arc::new(Mutex::new(
        Governance::new(
            DefaultPdp::new(
                PolicySet::new(vec![Rule {
                    id: RuleId("read".into()),
                    role: Role("analyst".into()),
                    action: Action("Read".into()),
                    purpose: None,
                    selector: Selector::Any,
                    effect: Effect::Permit,
                }])
                .unwrap(),
            ),
            &mut SeedRng(77),
        )
        .unwrap(),
    ));
    let gov_route = gov.clone();

    let router = Router::new()
        .route(
            "fabric.get",
            Arc::new(|_peer: &WorkloadIdentity, payload: &[u8]| {
                let mut v = b"fabric:".to_vec();
                v.extend_from_slice(payload);
                v
            }),
        )
        .route(
            "governance.authorize",
            Arc::new(move |peer: &WorkloadIdentity, _payload: &[u8]| {
                let ctx = GovContext {
                    subject: peer.id.0.clone(),
                    roles: peer.roles.clone(),
                    clearance: Clearance {
                        level: ClassificationLevel::UNCLASSIFIED,
                        compartments: BTreeSet::new(),
                    },
                    purpose: Purpose("investigation".into()),
                };
                let resource = Resource {
                    id: ResourceId("case-42".into()),
                    marking: Marking {
                        level: ClassificationLevel::UNCLASSIFIED,
                        compartments: BTreeSet::new(),
                    },
                    allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
                };
                let mut g = gov_route.lock().unwrap();
                let d = g.authorize(&ctx, &resource, "Read", 1_000).unwrap();
                // Return the real audit sequence so the test can prove the request
                // reached governance and was audited.
                format!("permitted={} auditSeq={}", d.permitted, d.audit_seq).into_bytes()
            }),
        )
        .route(
            "kms.seal",
            Arc::new(|_peer: &WorkloadIdentity, _payload: &[u8]| b"kms-sealed".to_vec()),
        );
    (Arc::new(router), gov)
}

/// Spawn a gateway accept loop on a fresh port; returns its address. The listener
/// thread runs for the test's lifetime (not joined).
fn spawn(gateway: Gateway) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let gw = gateway.clone();
            thread::spawn(move || {
                let _ = gw.handle_connection(stream);
            });
        }
    });
    addr
}

/// [`spawn`], also reporting each connection's outcome on the returned channel.
fn spawn_observed(
    gateway: Gateway,
) -> (
    std::net::SocketAddr,
    mpsc::Receiver<Result<WorkloadIdentity, MtlsError>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let gw = gateway.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let _ = tx.send(gw.handle_connection(stream));
            });
        }
    });
    (addr, rx)
}

/// The next connection outcome the gateway reports.
fn outcome(rx: &mpsc::Receiver<Result<WorkloadIdentity, MtlsError>>) -> Result<(), MtlsError> {
    rx.recv_timeout(Duration::from_secs(30))
        .expect("the gateway reports every connection")
        .map(|_| ())
}

/// A clock the test moves by hand.
fn manual_clock(start: u64) -> (Arc<AtomicU64>, Arc<dyn Fn() -> u64 + Send + Sync>) {
    let now = Arc::new(AtomicU64::new(start));
    let read = now.clone();
    (now, Arc::new(move || read.load(Ordering::SeqCst)))
}

// ---- client ---------------------------------------------------------------

/// A connected client session over the record layer.
struct Client {
    stream: TcpStream,
    sealer: pq_mtls::RecordSealer,
    opener: pq_mtls::RecordOpener,
    ticket: Option<ClientTicket>,
    /// The raw frame the ticket arrived in, as it crossed the wire.
    ticket_frame: Vec<u8>,
    suite: KemSuiteId,
}

impl Client {
    /// Open a full handshake, capturing the resumption ticket the gateway issues.
    fn connect_full(
        provider: &CryptoProvider,
        cert: IdentityCert,
        sk: &SigningKey,
        anchor: pq_mtls::TrustAnchor,
        addr: std::net::SocketAddr,
        seed: u64,
    ) -> Result<Client, MtlsError> {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(&[MODE_FULL]).unwrap();
        let mut ch = TcpChannel::new(stream);
        let mut rng = SeedRng(seed);
        let init = Initiator::new(
            provider,
            cert,
            sk,
            anchor,
            pq_mtls::ServiceId("svc-gateway".into()),
            Purpose("investigation".into()),
            posture(),
            &mut rng,
        );
        let session = run_initiator(init, &mut ch)?;
        let mut stream = ch.into_stream();
        let (sealer, mut opener) = session.records(RecordRole::Initiator);
        // The ticket is the first sealed record. [VLD-ANB-MIDGARD-0046]
        let ticket_frame = read_frame(&mut stream).map_err(MtlsError::Transport)?;
        let ticket_bytes = opener.open(NEW_TICKET_AAD, &ticket_frame)?;
        let ticket = Some(ClientTicket::new(&session, ticket_bytes));
        Ok(Client {
            stream,
            sealer,
            opener,
            ticket,
            ticket_frame,
            suite: session.suite(),
        })
    }

    /// Resume from a previously issued ticket (abbreviated handshake) at `now`.
    fn connect_resumed_at(
        addr: std::net::SocketAddr,
        ticket: &ClientTicket,
        anchor: &pq_mtls::TrustAnchor,
        now: u64,
        seed: u64,
    ) -> Result<Client, MtlsError> {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(&[MODE_RESUME]).unwrap();
        let mut rng = SeedRng(seed);
        let provider = fips_provider();
        let session = run_resume_initiator(&mut stream, ticket, anchor, &provider, now, &mut rng)?;
        let (sealer, opener) = session.records(RecordRole::Initiator);
        Ok(Client {
            stream,
            sealer,
            opener,
            ticket: None,
            ticket_frame: Vec::new(),
            suite: session.suite(),
        })
    }

    /// Resume at t=1000.
    fn connect_resumed(
        addr: std::net::SocketAddr,
        ticket: &ClientTicket,
        anchor: &pq_mtls::TrustAnchor,
        seed: u64,
    ) -> Result<Client, MtlsError> {
        Self::connect_resumed_at(addr, ticket, anchor, 1_000, seed)
    }

    /// Send one routed request and read the `(status, body)` response.
    fn call(&mut self, method: &str, payload: &[u8]) -> Result<(u8, Vec<u8>), MtlsError> {
        let req = encode_request(method, payload);
        write_frame(&mut self.stream, &self.sealer.seal(RECORD_AAD, &req)?)
            .map_err(MtlsError::Transport)?;
        let frame = read_frame(&mut self.stream).map_err(MtlsError::Transport)?;
        let resp = self.opener.open(RECORD_AAD, &frame)?;
        let (s, body) = decode_response(&resp).expect("response envelope");
        Ok((s, body.to_vec()))
    }

    /// Send a raw request frame and report whether a response came back (used to
    /// observe throttling, where the gateway drops the connection).
    fn try_call(&mut self, method: &str) -> bool {
        let req = encode_request(method, b"");
        let sealed = match self.sealer.seal(RECORD_AAD, &req) {
            Ok(s) => s,
            Err(_) => return false,
        };
        if write_frame(&mut self.stream, &sealed).is_err() {
            return false;
        }
        match read_frame(&mut self.stream) {
            Ok(f) => self.opener.open(RECORD_AAD, &f).is_ok(),
            Err(_) => false,
        }
    }
}

fn base_gateway(handler: Arc<dyn RequestHandler>) -> (Gateway, pq_mtls::TrustAnchor, Idp) {
    let idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let (gw_cert, gw_sk) = workload(&idp, "svc-gateway", &["server"], 2);
    let anchor = idp.trust_anchor();
    let pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(permit_pdp("analyst"));
    let gateway = Gateway::new(
        Arc::new(fips_provider()),
        gw_cert,
        Arc::new(gw_sk),
        anchor.clone(),
        pdp,
        channel_resource(),
        handler,
    );
    (gateway, anchor, idp)
}

#[test]
fn many_requests_share_one_handshake_and_route_correctly() {
    let (handler, _gov) = control_plane_router();
    let (gateway, anchor, idp) = base_gateway(handler);
    let gateway = gateway.with_resumption(TicketKey::generate(&mut SeedRng(9)), 3_600);
    let (cl_cert, cl_sk) = workload(&idp, "svc-client", &["analyst"], 3);
    let addr = spawn(gateway);

    let provider = fips_provider();
    let mut client = Client::connect_full(&provider, cl_cert, &cl_sk, anchor.clone(), addr, 5)
        .expect("full handshake");

    // Routing: each method reaches its own handler.
    let (s, body) = client.call("fabric.get", b"object-7").unwrap();
    assert_eq!(s, status::OK);
    assert_eq!(body, b"fabric:object-7");

    let (s, body) = client.call("governance.authorize", b"").unwrap();
    assert_eq!(s, status::OK);
    // Reached the REAL governance spine: a permit with a monotonic audit sequence.
    assert!(String::from_utf8_lossy(&body).starts_with("permitted=true auditSeq="));

    let (s, body) = client.call("kms.seal", b"blob").unwrap();
    assert_eq!(s, status::OK);
    assert_eq!(body, b"kms-sealed");

    // Unknown method is refused, never misrouted.
    let (s, _) = client.call("evil.exfiltrate", b"").unwrap();
    assert_eq!(s, status::NO_ROUTE);

    // Load: 50 more requests on the SAME session (one handshake amortised).
    for i in 0..50u32 {
        let (s, body) = client.call("fabric.get", &i.to_le_bytes()).unwrap();
        assert_eq!(s, status::OK);
        assert_eq!(&body[..7], b"fabric:");
    }
}

#[test]
fn a_second_connection_resumes_and_the_ticket_is_single_use() {
    let (handler, _gov) = control_plane_router();
    let (gateway, anchor, idp) = base_gateway(handler);
    let gateway = gateway.with_resumption(TicketKey::generate(&mut SeedRng(9)), 3_600);
    let (cl_cert, cl_sk) = workload(&idp, "svc-client", &["analyst"], 3);
    let addr = spawn(gateway);

    let provider = fips_provider();
    let mut full = Client::connect_full(&provider, cl_cert, &cl_sk, anchor.clone(), addr, 5)
        .expect("full handshake");
    let ticket = full.ticket.take().expect("gateway issued a ticket");
    // The full session still serves fine.
    assert_eq!(full.call("kms.seal", b"x").unwrap().0, status::OK);

    // Resume on a NEW connection — abbreviated handshake, no KEM/signatures.
    let mut resumed =
        Client::connect_resumed(addr, &ticket, &anchor, 11).expect("resumption succeeds");
    let (s, body) = resumed.call("fabric.get", b"post-resume").unwrap();
    assert_eq!(s, status::OK);
    assert_eq!(body, b"fabric:post-resume");

    // The ticket is single-use: a replay is rejected by the gateway.
    let replay = Client::connect_resumed(addr, &ticket, &anchor, 12);
    assert!(replay.is_err(), "a replayed ticket must be refused");
}

#[test]
fn an_expired_ticket_is_refused() {
    let (handler, _gov) = control_plane_router();
    let (gateway, anchor, idp) = base_gateway(handler);
    // An advancing clock: the ticket is issued at t=1000 (TTL 100s); we then push
    // the clock to t=2000 before resuming, so the gateway sees an expired ticket.
    let now = Arc::new(AtomicU64::new(1_000));
    let now_read = now.clone();
    let clock: Arc<dyn Fn() -> u64 + Send + Sync> =
        Arc::new(move || now_read.load(Ordering::SeqCst));
    let gateway = gateway
        .with_resumption(TicketKey::generate(&mut SeedRng(9)), 100)
        .with_clock(clock);
    let (cl_cert, cl_sk) = workload(&idp, "svc-client", &["analyst"], 3);
    let addr = spawn(gateway);

    let provider = fips_provider();
    // connect_full blocks until the ticket frame arrives, so it is issued at t=1000.
    let mut full = Client::connect_full(&provider, cl_cert, &cl_sk, anchor.clone(), addr, 5)
        .expect("full handshake");
    let ticket = full.ticket.take().expect("ticket issued");

    // Age past the TTL, then resume → refused.
    now.store(2_000, Ordering::SeqCst);
    let expired = Client::connect_resumed(addr, &ticket, &anchor, 11);
    assert!(expired.is_err(), "an expired ticket must be refused");
}

#[test]
fn an_abusive_client_is_rate_limited() {
    let (handler, _gov) = control_plane_router();
    let (gateway, anchor, idp) = base_gateway(handler);
    // 3-token bucket, no refill: a hard quota of 3 requests per identity.
    // (Resumption is enabled so the client's connection-mode byte is consumed.)
    let gateway = gateway
        .with_resumption(TicketKey::generate(&mut SeedRng(9)), 3_600)
        .with_rate_limiter(RateLimiter::new(3.0, 0.0));
    let (cl_cert, cl_sk) = workload(&idp, "svc-client", &["analyst"], 3);
    let addr = spawn(gateway);

    let provider = fips_provider();
    let mut client = Client::connect_full(&provider, cl_cert, &cl_sk, anchor.clone(), addr, 5)
        .expect("full handshake");

    // First three requests are served …
    assert!(client.try_call("kms.seal"), "request 1 served");
    assert!(client.try_call("kms.seal"), "request 2 served");
    assert!(client.try_call("kms.seal"), "request 3 served");
    // … the fourth is throttled: the gateway drops the connection.
    assert!(!client.try_call("kms.seal"), "request 4 must be throttled");
}

// ---- VLD-ANB-MIDGARD-0046: resumption hardening ---------------------------

/// A resumption-capable gateway with a hand-moved clock, its CA, and a client.
fn resumption_rig(
    ttl: u64,
    start: u64,
    client_not_after: u64,
) -> (
    Gateway,
    pq_mtls::TrustAnchor,
    (IdentityCert, SigningKey),
    Arc<AtomicU64>,
) {
    let (handler, _gov) = control_plane_router();
    let (gateway, anchor, idp) = base_gateway(handler);
    let (now, clock) = manual_clock(start);
    let gateway = gateway
        .with_resumption(TicketKey::generate(&mut SeedRng(9)), ttl)
        .with_clock(clock);
    let client = workload_until(&idp, "svc-client", &["analyst"], 3, client_not_after);
    (gateway, anchor, client, now)
}

/// A ResumeHello for `ticket` whose binder is 32 zero bytes, laid out as
/// `run_resume_initiator` sends it: `u32 len || ticket || nonce_c || suite ||
/// u32 len || kem_pk || binder`.
fn resume_hello_with_zero_binder(ticket: &[u8], suite: KemSuiteId) -> Vec<u8> {
    let (kem_pk, _) = fips_provider()
        .kem_for(suite)
        .unwrap()
        .keypair(&mut SeedRng(40))
        .unwrap();
    let mut o = Vec::new();
    o.extend_from_slice(&(ticket.len() as u32).to_le_bytes());
    o.extend_from_slice(ticket);
    o.extend_from_slice(&[0x11; 32]);
    o.push(kem_pk.suite().as_u8());
    o.extend_from_slice(&(kem_pk.as_bytes().len() as u32).to_le_bytes());
    o.extend_from_slice(kem_pk.as_bytes());
    o.extend_from_slice(&[0u8; 32]);
    o
}

/// AC-52.1: the ticket crosses the wire only inside a sealed record, and the client
/// recovers it with its record opener. [VLD-ANB-MIDGARD-0046]
#[test]
fn ticket_is_delivered_inside_the_record_layer() {
    let (gateway, anchor, (cl_cert, cl_sk), _now) = resumption_rig(3_600, 1_000, u64::MAX);
    let addr = spawn(gateway);
    let provider = fips_provider();
    let mut client = Client::connect_full(&provider, cl_cert, &cl_sk, anchor.clone(), addr, 5)
        .expect("full handshake");
    let ticket = client
        .ticket
        .take()
        .expect("ticket recovered via the opener");
    let frame = &client.ticket_frame;
    assert!(ticket.ticket.len() > 32);
    assert!(
        !frame
            .windows(ticket.ticket.len())
            .any(|w| w == ticket.ticket.as_slice()),
        "the ticket must not appear in clear on the wire"
    );
    // Both record streams stay in step after the ticket record.
    assert_eq!(client.call("kms.seal", b"x").unwrap().0, status::OK);
    // And the recovered ticket resumes.
    let mut resumed = Client::connect_resumed(addr, &ticket, &anchor, 11).expect("resumes");
    assert_eq!(resumed.call("kms.seal", b"y").unwrap().0, status::OK);
}

/// AC-52.2: a ResumeHello with a bad binder is refused without consuming the ticket;
/// the real resumption then succeeds, and presenting the ticket again is refused, in
/// that order. [VLD-ANB-MIDGARD-0046]
#[test]
fn a_bad_binder_does_not_consume_the_ticket() {
    let (gateway, anchor, (cl_cert, cl_sk), _now) = resumption_rig(3_600, 1_000, u64::MAX);
    let (addr, results) = spawn_observed(gateway);
    let provider = fips_provider();
    let mut full = Client::connect_full(&provider, cl_cert, &cl_sk, anchor.clone(), addr, 5)
        .expect("full handshake");
    let ticket = full.ticket.take().unwrap();
    let suite = full.suite;
    drop(full);
    let _ = outcome(&results); // the full connection, cut by the drop

    // 1. Right ticket, binder of zeros.
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(&[MODE_RESUME]).unwrap();
    write_frame(
        &mut stream,
        &resume_hello_with_zero_binder(&ticket.ticket, suite),
    )
    .unwrap();
    assert!(read_frame(&mut stream).is_err(), "no ResumeAccept");
    assert!(matches!(
        outcome(&results),
        Err(MtlsError::KeyConfirmationFailed)
    ));

    // 2. The real resumption with the same ticket still succeeds.
    let mut resumed = Client::connect_resumed(addr, &ticket, &anchor, 11).expect("resumes");
    assert_eq!(resumed.call("kms.seal", b"x").unwrap().0, status::OK);
    drop(resumed);
    let _ = outcome(&results);

    // 3. And it was single-use.
    assert!(Client::connect_resumed(addr, &ticket, &anchor, 12).is_err());
    assert!(matches!(
        outcome(&results),
        Err(MtlsError::Authz(pq_mtls::AuthzError::Denied))
    ));
}

/// AC-52.2: two correct resumptions with one ticket: the second is Denied.
/// [VLD-ANB-MIDGARD-0046]
#[test]
fn a_ticket_presented_twice_is_refused() {
    let (gateway, anchor, (cl_cert, cl_sk), _now) = resumption_rig(3_600, 1_000, u64::MAX);
    let (addr, results) = spawn_observed(gateway);
    let provider = fips_provider();
    let mut full = Client::connect_full(&provider, cl_cert, &cl_sk, anchor.clone(), addr, 5)
        .expect("full handshake");
    let ticket = full.ticket.take().unwrap();
    drop(full);
    let _ = outcome(&results);

    let first = Client::connect_resumed(addr, &ticket, &anchor, 11).expect("first resumes");
    drop(first);
    let _ = outcome(&results);
    assert!(Client::connect_resumed(addr, &ticket, &anchor, 12).is_err());
    assert!(matches!(
        outcome(&results),
        Err(MtlsError::Authz(pq_mtls::AuthzError::Denied))
    ));
}

/// AC-52.4: a client whose certificate is revoked after its ticket was issued, or
/// that has reached its not_after, cannot resume; the gateway says why.
/// [VLD-ANB-MIDGARD-0046] [MG-SEC-02] [MG-SEC-03]
#[test]
fn a_revoked_or_expired_client_cert_cannot_resume() {
    // Revoked in the gateway's (shared, live) anchor after issue.
    let (gateway, anchor, (cl_cert, cl_sk), _now) = resumption_rig(3_600, 1_000, u64::MAX);
    let fingerprint = cl_cert.fingerprint();
    let (addr, results) = spawn_observed(gateway);
    let provider = fips_provider();
    let mut full = Client::connect_full(&provider, cl_cert, &cl_sk, anchor.clone(), addr, 5)
        .expect("full handshake");
    let ticket = full.ticket.take().unwrap();
    drop(full);
    let _ = outcome(&results);
    anchor.revoke(fingerprint);
    assert!(Client::connect_resumed(addr, &ticket, &anchor, 11).is_err());
    assert!(matches!(outcome(&results), Err(MtlsError::PeerCertRevoked)));

    // Expired: the certificate ends at t=5000 and the ticket is cut to end with it;
    // at t=5000 the ticket is still in date but the certificate is not.
    let (gateway, anchor, (cl_cert, cl_sk), now) = resumption_rig(10_000, 1_000, 5_000);
    let (addr, results) = spawn_observed(gateway);
    let mut full = Client::connect_full(&provider, cl_cert, &cl_sk, anchor.clone(), addr, 5)
        .expect("full handshake");
    let ticket = full.ticket.take().unwrap();
    drop(full);
    let _ = outcome(&results);
    now.store(5_000, Ordering::SeqCst);
    assert!(Client::connect_resumed_at(addr, &ticket, &anchor, 5_000, 11).is_err());
    assert!(matches!(
        outcome(&results),
        Err(MtlsError::PeerCertExpired { .. })
    ));
}

/// AC-52.5: the seen-ticket set never exceeds its cap, refuses beyond it, and drops
/// entries once they expire. [VLD-ANB-MIDGARD-0046]
#[test]
fn seen_tickets_are_pruned_and_capped() {
    const TTL: u64 = 100;
    let mut seen = SeenTickets::new();
    let now = 1_000;
    let mut full = 0usize;
    for i in 0..70_000u32 {
        match seen.admit(&i.to_le_bytes(), now + TTL, now) {
            TicketAdmission::Fresh => {}
            TicketAdmission::Full => full += 1,
            TicketAdmission::Replayed => panic!("every synthetic ticket is distinct"),
        }
        assert!(seen.len() <= MAX_SEEN_TICKETS);
    }
    assert_eq!(seen.len(), MAX_SEEN_TICKETS);
    assert_eq!(full, 70_000 - MAX_SEEN_TICKETS);
    // A ticket already seen is a replay, not a fresh one.
    assert_eq!(
        seen.admit(&0u32.to_le_bytes(), now + TTL, now),
        TicketAdmission::Replayed
    );
    // Still in date at the expiry second: nothing is pruned.
    assert_eq!(
        seen.admit(b"late", now + 2 * TTL, now + TTL),
        TicketAdmission::Full
    );
    // Past the expiry everything is pruned and the set takes tickets again.
    assert_eq!(
        seen.admit(b"later", now + 3 * TTL, now + TTL + 1),
        TicketAdmission::Fresh
    );
    assert_eq!(seen.len(), 1);
}

/// AC-52.5: ticket keys rotate once per TTL, and a ticket issued under the previous
/// key still resumes while it is in date. [VLD-ANB-MIDGARD-0046]
#[test]
fn ticket_keys_rotate_and_old_tickets_still_open_within_ttl() {
    let (gateway, anchor, (cl_cert, cl_sk), now) = resumption_rig(100, 1_000, u64::MAX);
    let addr = spawn(gateway);
    let provider = fips_provider();
    let ticket_at = |t: u64, seed: u64| {
        now.store(t, Ordering::SeqCst);
        Client::connect_full(
            &provider,
            cl_cert.clone(),
            &cl_sk,
            anchor.clone(),
            addr,
            seed,
        )
        .expect("full handshake")
        .ticket
        .take()
        .unwrap()
    };
    let first = ticket_at(1_000, 5);
    let before_rotation = ticket_at(1_090, 6);
    let after_rotation = ticket_at(1_150, 7);
    assert_eq!(first.ticket[0], 0, "the first key's id");
    assert_eq!(before_rotation.ticket[0], 0, "no rotation within the TTL");
    assert_eq!(after_rotation.ticket[0], 1, "rotated past the TTL");

    // The previous key still opens a ticket that is in date …
    assert!(Client::connect_resumed_at(addr, &before_rotation, &anchor, 1_150, 11).is_ok());
    // … one past its TTL is refused …
    assert!(Client::connect_resumed_at(addr, &first, &anchor, 1_150, 12).is_err());
    // … and the current key's tickets resume.
    assert!(Client::connect_resumed_at(addr, &after_rotation, &anchor, 1_150, 13).is_ok());
}
