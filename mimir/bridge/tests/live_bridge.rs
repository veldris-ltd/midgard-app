//! RC-45 (transport): the live `PqMtlsClient` reaches a real inference gateway over
//! PQ-mTLS and returns a served completion — and fails closed (never fabricates a
//! result) on a policy denial or an unreachable server. Stands up the same gateway
//! + server the model-server's own PQ-mTLS test uses; here the *client* under test
//! is the production bridge. [FR-MI-8] [RT-45]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

use gateway::{Gateway, RequestHandler};
use governance::Governance;
use iam::{Idp, IDP_SIG_ALG};
use model_server::{
    reference_registry, InferenceHandler, Locality, LogicalClock, NodeProfile, Server,
};
use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicyDecisionPoint, PolicySet,
    Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{IdentityCert, Posture, ServiceId, TrustAnchor, WorkloadIdentity};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, SigningKey};

use mimir_bridge::{
    BridgeError, Inference, InferenceRequest, MimirEndpoint, PqMtlsClient, WorkloadClass,
};

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

fn deny_all() -> DefaultPdp {
    DefaultPdp::new(PolicySet::new(vec![]).unwrap())
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

fn posture() -> Posture {
    Posture {
        level: ClassificationLevel::SECRET,
        compartments: BTreeSet::new(),
    }
}

fn build_gateway(
    channel_pdp: DefaultPdp,
    infer_pdp: DefaultPdp,
    gw_cert: IdentityCert,
    gw_sk: SigningKey,
    anchor: TrustAnchor,
) -> Gateway {
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
    let handler_dyn: Arc<dyn RequestHandler> = handler;
    Gateway::new(
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
    )
}

fn request() -> InferenceRequest {
    InferenceRequest {
        purpose: "investigation".into(),
        workload_class: WorkloadClass::Balanced,
        quant_pref: None,
        backend_override: None,
        max_tokens: 16,
        prompt: "who is assigned to case 42?".into(),
    }
}

fn endpoint(
    addr: String,
    cert: IdentityCert,
    sk: SigningKey,
    anchor: TrustAnchor,
) -> MimirEndpoint {
    MimirEndpoint {
        addr,
        anchor,
        cert,
        signing_key: sk,
        purpose: "investigation".into(),
        posture: posture(),
    }
}

#[test]
fn live_client_gets_a_completion_over_pqmtls() {
    let idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let (gw_cert, gw_sk) = workload(&idp, "svc-mimir", &["server"], 2);
    let (cl_cert, cl_sk) = workload(&idp, "svc-forge", &["inference-consumer"], 3);
    let anchor = idp.trust_anchor();

    let gateway = build_gateway(
        permit("inference-consumer", "OpenChannel"),
        permit("inference-consumer", "Infer"),
        gw_cert,
        gw_sk,
        anchor.clone(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    thread::scope(|s| {
        let server = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            gateway.handle_connection(stream)
        });

        let client = PqMtlsClient::new(endpoint(addr, cl_cert, cl_sk, anchor.clone()));
        let completion = client
            .infer(request())
            .expect("live inference over PQ-mTLS");

        // A real in-enclosure completion — no hosted dependency, and it decoded.
        assert_eq!(completion.locality, Locality::InEnclosure);
        assert_eq!(completion.model_id, "local-reference");
        assert!(
            !completion.text.is_empty(),
            "the model returned advisory text"
        );

        let served = server
            .join()
            .unwrap()
            .expect("gateway served the connection");
        assert_eq!(served.id, ServiceId("svc-forge".into()));
    });
}

#[test]
fn policy_denial_surfaces_as_fail_closed_denied() {
    let idp = Idp::generate("idp", &mut SeedRng(11)).unwrap();
    let (gw_cert, gw_sk) = workload(&idp, "svc-mimir", &["server"], 12);
    let (cl_cert, cl_sk) = workload(&idp, "svc-forge", &["inference-consumer"], 13);
    let anchor = idp.trust_anchor();

    // Channel permitted (handshake succeeds) but inference is denied by policy.
    let gateway = build_gateway(
        permit("inference-consumer", "OpenChannel"),
        deny_all(),
        gw_cert,
        gw_sk,
        anchor.clone(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    thread::scope(|s| {
        let server = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            let _ = gateway.handle_connection(stream);
        });

        let client = PqMtlsClient::new(endpoint(addr, cl_cert, cl_sk, anchor.clone()));
        let err = client
            .infer(request())
            .expect_err("a denied call must not yield a completion");
        assert!(matches!(err, BridgeError::Denied(_)), "unexpected: {err:?}");

        let _ = server.join();
    });
}

#[test]
fn unreachable_server_fails_closed_as_transport() {
    // Bind then drop the listener so the port is closed — connect must fail closed.
    let addr = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().to_string()
    };
    let idp = Idp::generate("idp", &mut SeedRng(21)).unwrap();
    let (cl_cert, cl_sk) = workload(&idp, "svc-forge", &["inference-consumer"], 23);
    let anchor = idp.trust_anchor();

    let client = PqMtlsClient::new(endpoint(addr, cl_cert, cl_sk, anchor));
    let err = client
        .infer(request())
        .expect_err("an unreachable Mimir must fail closed");
    assert!(
        matches!(err, BridgeError::Transport(_)),
        "unexpected: {err:?}"
    );
}
