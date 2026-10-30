//! The gateway terminates PQ-mTLS over real TCP: it authenticates IdP-certified
//! peers (AC-M-3), negotiates the hybrid PQ suite (AC-M-1), authorises the
//! channel, and fails closed on unauthorised or untrusted peers.
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use gateway::{EchoHandler, Gateway, RECORD_AAD};
use iam::{Idp, IDP_SIG_ALG};
use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicyDecisionPoint, PolicySet,
    Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{
    read_frame, run_initiator, write_frame, Initiator, MtlsError, Posture, Role as RecordRole,
    ServiceId, TcpChannel, WorkloadIdentity,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{CryptoProvider, KemSuiteId, Rng, SigningKey};

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

/// Generate a workload keypair and have `idp` certify its identity.
fn workload(idp: &Idp, id: &str, roles: &[&str], seed: u64) -> (pq_mtls::IdentityCert, SigningKey) {
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
                u64::MAX,
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

fn deny_pdp() -> DefaultPdp {
    DefaultPdp::new(PolicySet::new(vec![]).unwrap())
}

fn posture() -> Posture {
    Posture {
        level: ClassificationLevel::SECRET,
        compartments: BTreeSet::new(),
    }
}

fn client_connect(
    provider: &CryptoProvider,
    cert: pq_mtls::IdentityCert,
    signing_key: &SigningKey,
    anchor: pq_mtls::TrustAnchor,
    addr: std::net::SocketAddr,
    seed: u64,
) -> Result<(KemSuiteId, ServiceId, Option<Vec<u8>>), MtlsError> {
    let stream = TcpStream::connect(addr).unwrap();
    let mut ch = TcpChannel::new(stream);
    let mut rng = SeedRng(seed);
    let init = Initiator::new(
        provider,
        cert,
        signing_key,
        anchor,
        pq_mtls::ServiceId("svc-gateway".into()),
        Purpose("investigation".into()),
        posture(),
        &mut rng,
    );
    let session = run_initiator(init, &mut ch)?;
    let suite = session.suite();
    let peer = session.peer().id.clone();
    let mut stream = ch.into_stream();
    let (mut seal, mut open) = session.records(RecordRole::Initiator);
    write_frame(
        &mut stream,
        &seal.seal(RECORD_AAD, b"hello gateway").unwrap(),
    )
    .map_err(MtlsError::Transport)?;
    let echoed = open
        .open(
            RECORD_AAD,
            &read_frame(&mut stream).map_err(MtlsError::Transport)?,
        )
        .ok();
    gateway::close_stream(&mut stream, &mut seal)?;
    Ok((suite, peer, echoed))
}

#[test]
fn authenticated_client_completes_and_is_echoed() {
    let mut idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let _ = idp.store_mut(); // store present; not needed for cert-based mTLS
    let (gw_cert, gw_sk) = workload(&idp, "svc-gateway", &["server"], 2);
    let (cl_cert, cl_sk) = workload(&idp, "svc-client", &["analyst"], 3);
    let anchor = idp.trust_anchor();

    let pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(permit_pdp("analyst"));
    let gateway = Gateway::new(
        Arc::new(fips_provider()),
        gw_cert,
        Arc::new(gw_sk),
        anchor.clone(),
        pdp,
        channel_resource(),
        Arc::new(EchoHandler),
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::scope(|s| {
        let server = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            gateway.handle_connection(stream)
        });

        let client_provider = fips_provider();
        let (suite, peer, echoed) =
            client_connect(&client_provider, cl_cert, &cl_sk, anchor.clone(), addr, 5).unwrap();

        // AC-M-1: hybrid X25519+ML-KEM-768 negotiated.
        assert_eq!(suite, KemSuiteId::HybridX25519MlKem768);
        // AC-M-3: the client authenticated the gateway's identity.
        assert_eq!(peer, ServiceId("svc-gateway".into()));
        // The echo came back over the record layer.
        assert_eq!(echoed.as_deref(), Some(&b"hello gateway"[..]));

        // The gateway authenticated the client's identity.
        let served = server
            .join()
            .unwrap()
            .expect("gateway served the connection");
        assert_eq!(served.id, ServiceId("svc-client".into()));
    });
}

#[test]
fn unauthorized_client_is_denied() {
    let idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let (gw_cert, gw_sk) = workload(&idp, "svc-gateway", &["server"], 2);
    let (cl_cert, cl_sk) = workload(&idp, "svc-client", &["analyst"], 3);
    let anchor = idp.trust_anchor();

    // Deny-by-default policy: the authenticated client is still refused.
    let pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(deny_pdp());
    let gateway = Gateway::new(
        Arc::new(fips_provider()),
        gw_cert,
        Arc::new(gw_sk),
        anchor.clone(),
        pdp,
        channel_resource(),
        Arc::new(EchoHandler),
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::scope(|s| {
        let server = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            gateway.handle_connection(stream)
        });
        let client_provider = fips_provider();
        let _ = client_connect(&client_provider, cl_cert, &cl_sk, anchor.clone(), addr, 5);
        let err = server.join().unwrap().unwrap_err();
        assert!(
            matches!(err, MtlsError::Authz(_)),
            "expected authz denial, got {err:?}"
        );
    });
}

#[test]
fn client_with_untrusted_cert_is_rejected() {
    let idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let rogue = Idp::generate("rogue-idp", &mut SeedRng(999)).unwrap();
    let (gw_cert, gw_sk) = workload(&idp, "svc-gateway", &["server"], 2);
    // The client's cert is signed by a DIFFERENT (untrusted) IdP.
    let (cl_cert, cl_sk) = workload(&rogue, "svc-client", &["analyst"], 3);
    let anchor = idp.trust_anchor();

    let pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(permit_pdp("analyst"));
    let gateway = Gateway::new(
        Arc::new(fips_provider()),
        gw_cert,
        Arc::new(gw_sk),
        anchor.clone(),
        pdp,
        channel_resource(),
        Arc::new(EchoHandler),
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::scope(|s| {
        let server = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            gateway.handle_connection(stream)
        });
        let client_provider = fips_provider();
        // The client trusts the real IdP (so it accepts the gateway's cert and
        // proceeds), but presents a cert signed by the rogue IdP — which the
        // gateway does not trust, so the gateway rejects it.
        let _ = client_connect(&client_provider, cl_cert, &cl_sk, anchor.clone(), addr, 5);
        let err = server.join().unwrap().unwrap_err();
        assert!(
            matches!(err, MtlsError::PeerCertUntrusted),
            "expected untrusted-cert rejection, got {err:?}"
        );
    });
}
