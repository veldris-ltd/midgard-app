//! The mesh listener is bounded: past its connection cap a connection is closed at once
//! and counted, a connection that sends nothing is closed at the timeout, the gateway's
//! per-identity rate limit is live once the service's limits are applied, and the
//! options that tune the limits refuse what is not a number. Each test serves a real
//! PQ-mTLS gateway on its own ephemeral port. [NFR-SEC-1] [VLD-ANB-MIDGARD-0004]
//! [AC-31.2] [AC-31.3] [AC-31.4] [AC-31.7]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use gateway::{EchoHandler, Gateway, RECORD_AAD};
use iam::{Idp, IDP_SIG_ALG};
use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicyDecisionPoint, PolicySet,
    Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{
    read_frame, run_initiator, write_frame, IdentityCert, Initiator, Posture, Role as RecordRole,
    ServiceId, TcpChannel, TrustAnchor, WorkloadIdentity,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, SigningKey};
use service_runtime::{
    apply_mesh_limits, mesh_limits_from_args, serve_mesh_with, Args, ListenerLimits, MeshLimits,
};
use telemetry::MetricsRegistry;

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
    let terms = pq_mtls::CertTerms::new(1, 0, u64::MAX, Posture::at(ClassificationLevel::SECRET));
    (idp.certify(&identity, terms).unwrap(), sk)
}

/// A gateway serving `svc-gateway` to `analyst` clients, echoing what they send, and a
/// client's certificate, key and trust anchor.
struct Fixture {
    gateway: Gateway,
    cert: IdentityCert,
    key: SigningKey,
    anchor: TrustAnchor,
}

fn fixture() -> Fixture {
    let idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let (gw_cert, gw_sk) = workload(&idp, "svc-gateway", &["server"], 2);
    let (cert, key) = workload(&idp, "svc-client", &["analyst"], 3);
    let anchor = idp.trust_anchor();
    let pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("r".into()),
            role: Role("analyst".into()),
            action: Action("OpenChannel".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    ));
    let resource = Resource {
        id: ResourceId("channel".into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    };
    let gateway = Gateway::new(
        Arc::new(fips_provider()),
        gw_cert,
        Arc::new(gw_sk),
        anchor.clone(),
        pdp,
        resource,
        Arc::new(EchoHandler),
    );
    Fixture {
        gateway,
        cert,
        key,
        anchor,
    }
}

/// Serve `gateway` within `limits` on a fresh loopback port, as a service binary does.
fn serve(gateway: Gateway, limits: ListenerLimits, metrics: Arc<MetricsRegistry>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || serve_mesh_with("test", listener, gateway, metrics, limits));
    addr
}

/// An established client session: the stream and its record sealer and opener.
struct Session {
    stream: TcpStream,
    seal: pq_mtls::RecordSealer,
    open: pq_mtls::RecordOpener,
}

impl Session {
    fn connect(f: &Fixture, addr: SocketAddr) -> Result<Session, pq_mtls::MtlsError> {
        let provider = fips_provider();
        let mut ch = TcpChannel::new(TcpStream::connect(addr).unwrap());
        let mut rng = SeedRng(5);
        let init = Initiator::new(
            &provider,
            f.cert.clone(),
            &f.key,
            f.anchor.clone(),
            ServiceId("svc-gateway".into()),
            Purpose("investigation".into()),
            Posture {
                level: ClassificationLevel::SECRET,
                compartments: BTreeSet::new(),
            },
            &mut rng,
        );
        let session = run_initiator(init, &mut ch)?;
        let stream = ch.into_stream();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (seal, open) = session.records(RecordRole::Initiator);
        Ok(Session { stream, seal, open })
    }

    /// Send one request; whether its echo came back.
    fn call(&mut self, body: &[u8]) -> bool {
        let Ok(sealed) = self.seal.seal(RECORD_AAD, body) else {
            return false;
        };
        if write_frame(&mut self.stream, &sealed).is_err() {
            return false;
        }
        match read_frame(&mut self.stream) {
            Ok(frame) => self.open.open(RECORD_AAD, &frame).is_ok_and(|b| b == body),
            Err(_) => false,
        }
    }
}

/// Whether the server closes `stream` (end of stream or a reset) within `within`.
fn closed_within(stream: &mut TcpStream, within: Duration) -> bool {
    stream
        .set_read_timeout(Some(within + Duration::from_secs(2)))
        .unwrap();
    let started = Instant::now();
    let mut buf = [0u8; 64];
    let closed = match stream.read(&mut buf) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => !matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
    };
    closed && started.elapsed() < within
}

fn wait_for(what: &str, within: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while !done() {
        assert!(Instant::now() < deadline, "{what} within {within:?}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_mesh_cap_refuses_the_extra_connection_and_counts_it() {
    // [VLD-ANB-MIDGARD-0004] [AC-31.3]
    let f = fixture();
    let metrics = Arc::new(MetricsRegistry::new());
    let limits = ListenerLimits {
        max_connections: 4,
        ..ListenerLimits::mesh()
    };
    let addr = serve(f.gateway.clone(), limits, metrics.clone());

    let mut idle: Vec<TcpStream> = (0..4).map(|_| TcpStream::connect(addr).unwrap()).collect();
    let mut fifth = TcpStream::connect(addr).unwrap();
    assert!(
        closed_within(&mut fifth, Duration::from_secs(2)),
        "the fifth connection is closed at once"
    );
    let over = || metrics.counter_value("mesh_connections_total", &[("outcome", "over_capacity")]);
    wait_for("the refusal is counted", Duration::from_secs(1), || {
        over() == 1
    });

    // A slot comes free when an idle connection goes, and a real peer is served.
    drop(idle.pop());
    let mut served = None;
    wait_for("a handshake succeeds", Duration::from_secs(5), || {
        served = Session::connect(&f, addr).ok();
        served.is_some()
    });
    assert!(served.unwrap().call(b"hello"), "the peer is served");
}

#[test]
fn an_unauthenticated_idle_mesh_connection_is_closed_at_the_timeout() {
    // [VLD-ANB-MIDGARD-0004] [AC-31.2]
    let f = fixture();
    let metrics = Arc::new(MetricsRegistry::new());
    let limits = ListenerLimits {
        read_timeout: Duration::from_secs(1),
        ..ListenerLimits::mesh()
    };
    let addr = serve(f.gateway.clone(), limits, metrics.clone());

    let mut silent = TcpStream::connect(addr).unwrap();
    assert!(
        closed_within(&mut silent, Duration::from_secs(3)),
        "a connection that sends nothing is closed at the read timeout"
    );
    wait_for("the refusal is counted", Duration::from_secs(1), || {
        metrics.counter_value("mesh_connections_total", &[("outcome", "refused")]) == 1
    });
    // The listener keeps serving.
    let mut session = Session::connect(&f, addr).expect("a handshake after the timeout");
    assert!(session.call(b"still here"));
}

#[test]
fn apply_mesh_limits_enforces_the_rate_limit() {
    // [VLD-ANB-MIDGARD-0004] [AC-31.4] [RT-25]
    let f = fixture();
    let limits = MeshLimits {
        rate_burst: 3.0,
        rate_per_sec: 0.0,
        ..MeshLimits::default()
    };
    let gateway = apply_mesh_limits(f.gateway.clone(), &limits);
    let addr = serve(gateway, limits.listener, Arc::new(MetricsRegistry::new()));

    let mut session = Session::connect(&f, addr).expect("a handshake");
    assert!(session.call(b"1"), "request 1 is served");
    assert!(session.call(b"2"), "request 2 is served");
    assert!(session.call(b"3"), "request 3 is served");
    assert!(
        !session.call(b"4"),
        "request 4 is throttled: the connection is dropped"
    );
}

#[test]
fn mesh_limits_from_args_refuses_junk() {
    // [VLD-ANB-MIDGARD-0004] [AC-31.7]
    const OPTIONS: [&str; 5] = [
        "max-connections",
        "idle-timeout",
        "rate-burst",
        "rate-per-sec",
        "requests-per-conn",
    ];
    let parse = |line: &str| {
        let argv: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        mesh_limits_from_args(&Args::parse(&argv, &OPTIONS, &[]).unwrap())
    };

    assert_eq!(parse(""), Ok(MeshLimits::default()));
    let tuned = parse(
        "--max-connections 64 --idle-timeout 10 --rate-burst 50 --rate-per-sec 10 \
         --requests-per-conn 1000",
    )
    .unwrap();
    assert_eq!(tuned.listener.max_connections, 64);
    assert_eq!(tuned.listener.read_timeout, Duration::from_secs(10));
    assert_eq!(tuned.listener.write_timeout, Duration::from_secs(10));
    assert_eq!((tuned.rate_burst, tuned.rate_per_sec), (50.0, 10.0));
    assert_eq!(tuned.requests_per_conn, 1000);

    let err = parse("--rate-per-sec abc").unwrap_err();
    assert!(err.contains("--rate-per-sec"), "{err}");
    for junk in [
        "--max-connections many",
        "--max-connections 0",
        "--max-connections -1",
        "--idle-timeout 1.5",
        "--idle-timeout 0",
        "--rate-burst NaN",
        "--rate-burst -3",
        "--rate-burst inf",
        "--requests-per-conn lots",
        "--rate-burst 0 --rate-per-sec 5",
    ] {
        assert!(parse(junk).is_err(), "{junk} must be refused");
    }
}
