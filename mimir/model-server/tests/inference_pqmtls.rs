//! AC-MI-8: a consuming product (Forge/Bifrost/Heimdall) calls inference over
//! internal PQ-mTLS and the call is audited. This drives real TCP sockets, the
//! same way the gateway's own acceptance test does, and additionally asserts a
//! PQ-signed audit entry is written for every call (permit and deny).
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use gateway::{Gateway, RequestHandler, RECORD_AAD};
use governance::Governance;
use iam::{Idp, IDP_SIG_ALG};
use model_server::{
    reference_registry, InferenceHandler, InferenceRequest, InferenceResponse, Locality,
    LogicalClock, NodeProfile, Server, WorkloadClass,
};
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
    (idp.certify(&identity).unwrap(), sk)
}

fn inference_resource() -> Resource {
    Resource {
        id: ResourceId("mimir-inference".into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

/// Permit `role` to perform `action` (used for both the channel and inference).
fn permit(role: &str, action: &str) -> DefaultPdp {
    DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("r".into()),
            role: Role(role.into()),
            action: Action(action.into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    )
}

fn posture() -> Posture {
    Posture {
        level: ClassificationLevel::SECRET,
        compartments: BTreeSet::new(),
    }
}

/// Build the inference gateway (PQ-mTLS termination + audited handler) and return
/// it alongside the handler handle (to read the audit ledger after the call).
fn build_service(
    channel_pdp: DefaultPdp,
    infer_pdp: DefaultPdp,
    gw_cert: pq_mtls::IdentityCert,
    gw_sk: SigningKey,
    anchor: pq_mtls::TrustAnchor,
) -> (Gateway, Arc<InferenceHandler>) {
    let registry = reference_registry();
    let server = Arc::new(Server::bootstrap(NodeProfile::cpu_only(4), &registry, None).unwrap());
    let governance = Governance::new(infer_pdp, &mut SeedRng(77)).unwrap();
    let handler = Arc::new(InferenceHandler::new(
        server,
        governance,
        inference_resource(),
        Arc::new(LogicalClock::starting_at(1000)),
    ));

    let channel_pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(channel_pdp);
    let handler_dyn: Arc<dyn RequestHandler> = handler.clone();
    let gateway = Gateway::new(
        Arc::new(fips_provider()),
        gw_cert,
        Arc::new(gw_sk),
        anchor,
        channel_pdp,
        Resource {
            id: ResourceId("channel".into()),
            marking: Marking {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
        },
        handler_dyn,
    );
    (gateway, handler)
}

/// Connect as a consumer, send one inference request, return the decoded response
/// and the negotiated KEM suite.
fn call_inference(
    provider: &CryptoProvider,
    cert: pq_mtls::IdentityCert,
    signing_key: &SigningKey,
    anchor: pq_mtls::TrustAnchor,
    addr: std::net::SocketAddr,
    request: &InferenceRequest,
    seed: u64,
) -> Result<(KemSuiteId, InferenceResponse), MtlsError> {
    let stream = TcpStream::connect(addr).unwrap();
    let mut ch = TcpChannel::new(stream);
    let mut rng = SeedRng(seed);
    let init = Initiator::new(
        provider,
        cert,
        signing_key,
        anchor,
        Purpose("investigation".into()),
        posture(),
        &mut rng,
    );
    let session = run_initiator(init, &mut ch)?;
    let suite = session.suite();
    let mut stream = ch.into_stream();
    let (mut seal, mut open) = session.records(RecordRole::Initiator);
    write_frame(
        &mut stream,
        &seal.seal(RECORD_AAD, &request.encode()).unwrap(),
    )
    .map_err(MtlsError::Transport)?;
    let raw = open
        .open(
            RECORD_AAD,
            &read_frame(&mut stream).map_err(MtlsError::Transport)?,
        )
        .map_err(|_| MtlsError::Protocol("open failed".into()))?;
    let resp = InferenceResponse::decode(&raw).expect("decodes");
    Ok((suite, resp))
}

fn request() -> InferenceRequest {
    InferenceRequest {
        purpose: "investigation".to_string(),
        workload_class: WorkloadClass::Balanced,
        quant_pref: None,
        backend_override: None,
        max_tokens: 16,
        prompt: "who is assigned to case 42?".to_string(),
        context: None,
    }
}

#[test]
fn ac_mi_8_consumer_calls_inference_over_pqmtls_and_is_audited() {
    let idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let (gw_cert, gw_sk) = workload(&idp, "svc-mimir", &["server"], 2);
    let (cl_cert, cl_sk) = workload(&idp, "svc-forge", &["inference-consumer"], 3);
    let anchor = idp.trust_anchor();

    let (gateway, handler) = build_service(
        permit("inference-consumer", "OpenChannel"),
        permit("inference-consumer", "Infer"),
        gw_cert,
        gw_sk,
        anchor.clone(),
    );
    assert_eq!(handler.audit_len(), 0, "no calls yet");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::scope(|s| {
        let server = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            gateway.handle_connection(stream)
        });

        let provider = fips_provider();
        let (suite, resp) = call_inference(
            &provider,
            cl_cert,
            &cl_sk,
            anchor.clone(),
            addr,
            &request(),
            5,
        )
        .unwrap();

        // The call rode the hybrid post-quantum channel.
        assert_eq!(suite, KemSuiteId::HybridX25519MlKem768);

        // A local completion came back — no hosted dependency.
        match resp {
            InferenceResponse::Ok(c) => {
                assert_eq!(c.locality, Locality::InEnclosure);
                assert_eq!(c.model_id, "local-reference");
                assert!(!c.text.is_empty());
                // The response carries the audit seq — evidence the call was recorded.
                assert_eq!(c.audit_seq, 0, "first audited call is seq 0");
            }
            InferenceResponse::Denied { reason, .. } => panic!("unexpected denial: {reason}"),
        }

        let served = server
            .join()
            .unwrap()
            .expect("gateway served the connection");
        assert_eq!(served.id, ServiceId("svc-forge".into()));
    });

    // Exactly one PQ-signed audit entry was written for the call. [AC-MI-8]
    assert_eq!(handler.audit_len(), 1, "every call writes one audit entry");
}

#[test]
fn denied_inference_is_still_audited() {
    let idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let (gw_cert, gw_sk) = workload(&idp, "svc-mimir", &["server"], 2);
    let (cl_cert, cl_sk) = workload(&idp, "svc-forge", &["inference-consumer"], 3);
    let anchor = idp.trust_anchor();

    // Channel opens, but inference itself is denied (no Infer rule).
    let (gateway, handler) = build_service(
        permit("inference-consumer", "OpenChannel"),
        DefaultPdp::new(PolicySet::new(vec![]).unwrap()),
        gw_cert,
        gw_sk,
        anchor.clone(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::scope(|s| {
        let server = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            gateway.handle_connection(stream)
        });
        let provider = fips_provider();
        let (_suite, resp) = call_inference(
            &provider,
            cl_cert,
            &cl_sk,
            anchor.clone(),
            addr,
            &request(),
            5,
        )
        .unwrap();
        match resp {
            InferenceResponse::Denied { reason, audit_seq } => {
                assert_eq!(reason, "POLICY_DENIED");
                assert_eq!(audit_seq, 0);
            }
            InferenceResponse::Ok(_) => panic!("expected policy denial"),
        }
        let _ = server.join().unwrap();
    });

    // A denied call is still audited — the envelope records permit AND deny.
    assert_eq!(handler.audit_len(), 1);
}
