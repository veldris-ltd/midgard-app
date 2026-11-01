//! RC-57: with connectivity, a Tier-2 request **delegates over a real PQ-mTLS
//! session** to the enclosure. Drives the production [`PqMtlsRemoteInference`]
//! bridge (the Initiator, via `EnclosureClient`) against a real PQ-mTLS Responder
//! over TCP sockets; the responder decodes the request and returns a grounded,
//! cited [`RemoteOutput`]. [RT-57] [FR-C-6] [AC-C-6]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::thread;

use midgard_client_enclosure::EnclosureClient;
use midgard_client_inference::{
    decode_request, encode_output, InferenceRequest, PqMtlsRemoteInference, RemoteInference,
    RemoteOutput, REMOTE_INFERENCE_AAD,
};
use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicySet, Purpose, Resource,
    ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{
    read_frame, run_responder, write_frame, IdentityCert, Posture, Responder, Role as RecordRole,
    ServiceId, TcpChannel, TrustAnchor, WorkloadIdentity,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{CryptoProvider, Rng, SigId, SigningKey, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;

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

fn keypair(p: &CryptoProvider, seed: u64) -> (VerifyingKey, SigningKey) {
    p.sig_for(ALG).unwrap().keypair(&mut SeedRng(seed)).unwrap()
}

fn ca(p: &CryptoProvider, seed: u64) -> (TrustAnchor, SigningKey) {
    let (vk, sk) = keypair(p, seed);
    (TrustAnchor::new(ALG, vk), sk)
}

fn party(
    p: &CryptoProvider,
    id: &str,
    roles: &[&str],
    seed: u64,
    ca_sk: &SigningKey,
) -> (IdentityCert, SigningKey) {
    let (vk, sk) = keypair(p, seed);
    let roles: BTreeSet<Role> = roles.iter().map(|r| Role(r.to_string())).collect();
    let identity = WorkloadIdentity {
        id: ServiceId(id.into()),
        roles,
        sig_alg: ALG,
        verifying_key: vk,
    };
    let ca_signature = p
        .sig_for(ALG)
        .unwrap()
        .sign(
            ca_sk,
            &pq_mtls::canonical_cert_body(&identity, &test_terms()),
        )
        .unwrap();
    (
        IdentityCert {
            identity,
            terms: test_terms(),
            ca_sig_alg: ALG,
            ca_signature,
        },
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
        level: ClassificationLevel::SECRET,
        compartments: BTreeSet::new(),
    }
}

#[test]
fn tier2_request_delegates_over_real_pqmtls() {
    let provider = fips_provider();
    let (anchor, ca_sk) = ca(&provider, 1);
    let (client_cert, client_sk) = party(&provider, "svc-client", &["assistant"], 10, &ca_sk);
    let (server_cert, server_sk) = party(&provider, "svc-enclosure", &["server"], 20, &ca_sk);
    let pdp = permit_pdp("assistant");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    thread::scope(|s| {
        // --- the enclosure side: a real PQ-mTLS responder that answers one request ---
        let server = s.spawn(|| {
            let provider = fips_provider();
            let (stream, _) = listener.accept().unwrap();
            let mut ch = TcpChannel::new(stream);
            let mut rng = SeedRng(2);
            let resp = Responder::new(
                &provider,
                server_cert.clone(),
                &server_sk,
                anchor.clone(),
                &pdp,
                channel_resource(),
                &mut rng,
            );
            let session = run_responder(resp, &mut ch).expect("responder handshake");
            let mut stream = ch.into_stream();
            let (mut seal, mut open) = session.records(RecordRole::Responder);

            // Decode the delegated request; ground + cite it as the enclosure would.
            let raw = open
                .open(REMOTE_INFERENCE_AAD, &read_frame(&mut stream).unwrap())
                .unwrap();
            let req = decode_request(&raw).expect("request decodes at the enclosure");
            let mut citations = req.local_context_ids.clone();
            if citations.is_empty() {
                citations.push("obj:enclosure-grounding".into());
            }
            let out = RemoteOutput {
                answer: format!("enclosure answer to: {}", req.user_message),
                citations,
                grounded: true,
            };
            write_frame(
                &mut stream,
                &seal
                    .seal(REMOTE_INFERENCE_AAD, &encode_output(&out))
                    .unwrap(),
            )
            .unwrap();
        });

        // --- the client side: the production bridge delegates over PQ-mTLS ---
        let enclosure = EnclosureClient::new(
            client_cert,
            anchor.clone(),
            pq_mtls::ServiceId("svc-enclosure".into()),
            Purpose("investigation".into()),
            posture(),
        );
        let bridge = PqMtlsRemoteInference::new(addr, enclosure, client_sk);

        let request = InferenceRequest::new("who is on call tonight?", vec!["obj:roster-7".into()]);
        let output = bridge
            .infer(&request)
            .expect("Tier-2 delegates over a real PQ-mTLS session");

        assert!(output.grounded, "the enclosure returned a grounded result");
        assert_eq!(
            output.citations,
            vec!["obj:roster-7".to_string()],
            "grounding citation round-trips"
        );
        assert!(
            output.answer.contains("who is on call tonight?"),
            "the answer addresses the request"
        );

        server.join().unwrap();
    });
}

/// Terms for test certificates: valid from the epoch to the end of time, granting
/// TOP SECRET, so a test's requested clearance is what it gets.
fn test_terms() -> pq_mtls::CertTerms {
    pq_mtls::CertTerms::new(
        1,
        0,
        u64::MAX,
        pq_mtls::Posture::at(policy::ClassificationLevel::TOP_SECRET),
    )
}
