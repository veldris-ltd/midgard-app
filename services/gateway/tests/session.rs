//! RC-25: the gateway is a real session boundary, not one-request-per-connection.
//!
//! - **Connection reuse:** one PQ-mTLS handshake serves many request/response
//!   records (the load test fires 50 on a single session).
//! - **Routing:** each request's method is dispatched to the right control-plane
//!   handler — including the *real* governance spine — and an unknown method is
//!   refused (never misrouted).
//! - **Resumption:** a second connection presents the ticket issued on the first
//!   and completes an abbreviated handshake (no KEM, no signatures); the ticket is
//!   single-use.
//! - **Rate limiting:** an abusive peer that exceeds its token bucket is throttled
//!   (its connection is dropped). [RC-25]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use gateway::{
    decode_response, encode_request, status, Gateway, RateLimiter, RequestHandler, Router,
    RECORD_AAD,
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
    WorkloadIdentity, MODE_FULL, MODE_RESUME,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{CryptoProvider, Rng, SigningKey};

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
    (idp.certify(&identity).unwrap(), sk)
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

// ---- client ---------------------------------------------------------------

/// A connected client session over the record layer.
struct Client {
    stream: TcpStream,
    sealer: pq_mtls::RecordSealer,
    opener: pq_mtls::RecordOpener,
    ticket: Option<ClientTicket>,
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
            Purpose("investigation".into()),
            posture(),
            &mut rng,
        );
        let session = run_initiator(init, &mut ch)?;
        let mut stream = ch.into_stream();
        let ticket_bytes = read_frame(&mut stream).map_err(MtlsError::Transport)?;
        let ticket = Some(ClientTicket::new(&session, ticket_bytes));
        let (sealer, opener) = session.records(RecordRole::Initiator);
        Ok(Client {
            stream,
            sealer,
            opener,
            ticket,
        })
    }

    /// Resume from a previously issued ticket (abbreviated handshake).
    fn connect_resumed(
        addr: std::net::SocketAddr,
        ticket: &ClientTicket,
        seed: u64,
    ) -> Result<Client, MtlsError> {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(&[MODE_RESUME]).unwrap();
        let mut rng = SeedRng(seed);
        let session = run_resume_initiator(&mut stream, ticket, &mut rng)?;
        let (sealer, opener) = session.records(RecordRole::Initiator);
        Ok(Client {
            stream,
            sealer,
            opener,
            ticket: None,
        })
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
    let mut client =
        Client::connect_full(&provider, cl_cert, &cl_sk, anchor, addr, 5).expect("full handshake");

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
    let mut full =
        Client::connect_full(&provider, cl_cert, &cl_sk, anchor, addr, 5).expect("full handshake");
    let ticket = full.ticket.clone().expect("gateway issued a ticket");
    // The full session still serves fine.
    assert_eq!(full.call("kms.seal", b"x").unwrap().0, status::OK);

    // Resume on a NEW connection — abbreviated handshake, no KEM/signatures.
    let mut resumed = Client::connect_resumed(addr, &ticket, 11).expect("resumption succeeds");
    let (s, body) = resumed.call("fabric.get", b"post-resume").unwrap();
    assert_eq!(s, status::OK);
    assert_eq!(body, b"fabric:post-resume");

    // The ticket is single-use: a replay is rejected by the gateway.
    let replay = Client::connect_resumed(addr, &ticket, 12);
    assert!(replay.is_err(), "a replayed ticket must be refused");
}

#[test]
fn an_expired_ticket_is_refused() {
    use std::sync::atomic::{AtomicU64, Ordering};
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
    let full =
        Client::connect_full(&provider, cl_cert, &cl_sk, anchor, addr, 5).expect("full handshake");
    let ticket = full.ticket.clone().expect("ticket issued");

    // Age past the TTL, then resume → refused.
    now.store(2_000, Ordering::SeqCst);
    let expired = Client::connect_resumed(addr, &ticket, 11);
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
    let mut client =
        Client::connect_full(&provider, cl_cert, &cl_sk, anchor, addr, 5).expect("full handshake");

    // First three requests are served …
    assert!(client.try_call("kms.seal"), "request 1 served");
    assert!(client.try_call("kms.seal"), "request 2 served");
    assert!(client.try_call("kms.seal"), "request 3 served");
    // … the fourth is throttled: the gateway drops the connection.
    assert!(!client.try_call("kms.seal"), "request 4 must be throttled");
}
