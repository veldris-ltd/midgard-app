//! RC-26 — Two Heimdall nodes over a real link. When a feed delivers a signed
//! observation to one node (over a real PQ-mTLS feed channel), the fused picture
//! converges on BOTH nodes (replicated over a real PQ-mTLS delta link), and an
//! unsigned / wrong-key feed is rejected at the boundary. [FR-H-3] [FR-H-6] [NFR-PQ-1]
//!
//! This exercises the real transports end to end:
//! - `heimdall-feed-gateway`'s `FeedClient` → `serve_feed_connection` over PQ-mTLS,
//! - `heimdall-picture-sync`'s `SessionLink` + `replicate_round` over PQ-mTLS,
//!
//! with signature verification and last-write-wins merge kept intact.
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::net::{TcpListener, TcpStream};
use std::thread;

use heimdall_feed_gateway::{
    serve_feed_connection, sign_observation, FeedClient, FeedGateway, FeedRegistry,
    VerifiedObservation,
};
use heimdall_fusion::FusionEngine;
use heimdall_picture::{Attr, EntityId, FeedId, Observation, Picture};
use heimdall_picture_sync::{replicate_round, Replicator, SessionLink};

use iam::{Idp, IDP_SIG_ALG};
use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicyDecisionPoint, PolicySet,
    Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{
    run_initiator, run_responder, IdentityCert, Initiator, Posture, Responder, Role as RecordRole,
    ServiceId, Session, TcpChannel, TrustAnchor, WorkloadIdentity,
};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::registry::{shared, CryptoProvider};
use pqc_agility::{Rng, SharedProvider, SigId, SignatureScheme, SigningKey, VerifyingKey};

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

fn shared_fips() -> SharedProvider {
    let mut p = CryptoProvider::new();
    p.register_sig(Box::new(FipsSig::new(ALG)));
    shared(p)
}

fn keypair(seed: u64) -> (VerifyingKey, SigningKey) {
    FipsSig::new(ALG).keypair(&mut SeedRng(seed)).unwrap()
}

/// A pq-mtls workload identity certified by the IdP (the channel identity).
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

/// A PDP permitting OpenChannel for each listed role.
fn permit_pdp(roles: &[&str]) -> DefaultPdp {
    let rules = roles
        .iter()
        .enumerate()
        .map(|(i, r)| Rule {
            id: RuleId(format!("r{i}")),
            role: Role((*r).into()),
            action: Action("OpenChannel".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        })
        .collect();
    DefaultPdp::new(PolicySet::new(rules).unwrap())
}

fn posture() -> Posture {
    Posture {
        level: ClassificationLevel::UNCLASSIFIED,
        compartments: BTreeSet::new(),
    }
}

/// Run the responder handshake over `stream`, returning the session + the raw stream.
fn responder_session(
    stream: TcpStream,
    cert: &IdentityCert,
    sk: &SigningKey,
    anchor: &TrustAnchor,
    pdp: &dyn PolicyDecisionPoint,
    seed: u64,
) -> (Session, TcpStream) {
    let provider = fips_provider();
    let mut rng = SeedRng(seed);
    let mut ch = TcpChannel::new(stream);
    let resp = Responder::new(
        &provider,
        cert.clone(),
        sk,
        anchor.clone(),
        pdp,
        channel_resource(),
        &mut rng,
    );
    let session = run_responder(resp, &mut ch).expect("responder handshake");
    (session, ch.into_stream())
}

/// Run the initiator handshake over `stream`, returning the session + the raw stream.
fn initiator_session(
    stream: TcpStream,
    cert: &IdentityCert,
    sk: &SigningKey,
    anchor: &TrustAnchor,
    peer: &str,
    seed: u64,
) -> (Session, TcpStream) {
    let provider = fips_provider();
    let mut rng = SeedRng(seed);
    let mut ch = TcpChannel::new(stream);
    let init = Initiator::new(
        &provider,
        cert.clone(),
        sk,
        anchor.clone(),
        pq_mtls::ServiceId(peer.into()),
        Purpose("investigation".into()),
        posture(),
        &mut rng,
    );
    let session = run_initiator(init, &mut ch).expect("initiator handshake");
    (session, ch.into_stream())
}

/// Build a radar observation of `key` at time `t`.
fn radar_obs(key: &str, t: u64, speed: f64) -> Observation {
    Observation::new(
        format!("radar-{key}-{t}"),
        FeedId("radar".into()),
        "air-track",
        key,
        t,
    )
    .with_attr("speed", Attr::Float(speed))
}

#[test]
fn rc_26_feed_over_pqmtls_converges_across_two_nodes() {
    // One IdP certifies every channel identity; nodes trust it as the mesh CA.
    let idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let anchor = idp.trust_anchor();

    // Channel identities: the feed source, node A (feed responder + repl initiator),
    // and node B (repl responder).
    let (feed_cert, feed_ch_sk) = workload(&idp, "feed-src", &["feed"], 2);
    // Node A has one channel identity for the feed listener and one for replication
    // (a workload can hold several certs; SigningKey is intentionally not Clone).
    let (a_feed_cert, a_feed_sk) = workload(&idp, "node-a", &["node"], 3);
    let (a_repl_cert, a_repl_sk) = workload(&idp, "node-a", &["node"], 30);
    let (b_cert, b_ch_sk) = workload(&idp, "node-b", &["node"], 4);

    // The feed's OBSERVATION-signing key (distinct from its channel key).
    let (feed_vk, feed_sk) = keypair(11);
    // The nodes' DELTA-signing keys.
    let (a_vk, a_sk) = keypair(20);
    let (b_vk, b_sk) = keypair(21);

    // ---- Phase 1: a feed delivers a signed observation to node A over PQ-mTLS ----
    let feed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let feed_addr = feed_listener.local_addr().unwrap();

    let feed_pdp = permit_pdp(&["feed"]);
    let anchor_srv = anchor.clone();
    // Node A's feed-gateway trusts feed "radar" → feed_vk.
    let mut registry = FeedRegistry::new();
    registry.register(FeedId("radar".into()), feed_vk);
    let gateway = FeedGateway::new(registry, shared_fips());

    let feed_server = thread::spawn(move || {
        let (stream, _) = feed_listener.accept().unwrap();
        let provider = fips_provider();
        let mut rng = SeedRng(101);
        let responder = Responder::new(
            &provider,
            a_feed_cert,
            &a_feed_sk,
            anchor_srv,
            &feed_pdp,
            channel_resource(),
            &mut rng,
        );
        let mut admitted: Vec<VerifiedObservation> = Vec::new();
        serve_feed_connection(stream, responder, &gateway, |v| admitted.push(v))
            .expect("feed connection served");
        admitted
    });

    // The feed source (SDK) connects and pushes one signed observation, then closes.
    {
        let stream = TcpStream::connect(feed_addr).unwrap();
        let provider = fips_provider();
        let mut rng = SeedRng(102);
        let init = Initiator::new(
            &provider,
            feed_cert,
            &feed_ch_sk,
            anchor.clone(),
            pq_mtls::ServiceId("node-a".into()),
            Purpose("investigation".into()),
            posture(),
            &mut rng,
        );
        let mut client = FeedClient::connect(stream, init).expect("feed client connects");
        let signed =
            sign_observation(&FipsSig::new(ALG), &feed_sk, radar_obs("trk-1", 100, 480.0)).unwrap();
        client.send(&signed).expect("feed sends observation");
        // client drops here → the listener's read loop sees EOF and returns.
    }

    let admitted = feed_server.join().unwrap();
    assert_eq!(
        admitted.len(),
        1,
        "exactly one observation admitted to node A"
    );

    // Node A fuses the admitted observation.
    let mut node_a = FusionEngine::new();
    for v in &admitted {
        node_a.ingest(v);
    }
    assert!(
        node_a.picture().entity(&EntityId("trk-1".into())).is_some(),
        "node A holds trk-1 after the feed"
    );
    // Node B has seen nothing yet.
    // ---- Phase 2: A and B replicate the picture over a real PQ-mTLS delta link ----
    let repl_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let repl_addr = repl_listener.local_addr().unwrap();

    let node_pdp = permit_pdp(&["node"]);
    let anchor_b = anchor.clone();

    // Node B: accept, establish session, replicate one round against node A.
    let node_b_thread = thread::spawn(move || {
        let (stream, _) = repl_listener.accept().unwrap();
        let (session, stream) =
            responder_session(stream, &b_cert, &b_ch_sk, &anchor_b, &node_pdp, 201);
        let mut link = SessionLink::new(&session, stream, RecordRole::Responder);
        let repl = Replicator::new(shared_fips());
        let mut picture = Picture::new();
        // B exports its (empty) picture, receives A's, verifies against A's key, merges.
        replicate_round(&mut link, &repl, &b_sk, &a_vk, &mut picture).expect("node B replicates");
        picture
    });

    // Node A (initiator): establish session and replicate against node B.
    let stream = TcpStream::connect(repl_addr).unwrap();
    let (session, stream) =
        initiator_session(stream, &a_repl_cert, &a_repl_sk, &anchor, "node-b", 202);
    let mut link_a = SessionLink::new(&session, stream, RecordRole::Initiator);
    let repl = Replicator::new(shared_fips());
    replicate_round(&mut link_a, &repl, &a_sk, &b_vk, node_a.picture_mut())
        .expect("node A replicates");

    let node_b_picture = node_b_thread.join().unwrap();

    // RC-26: the fused picture has converged on BOTH nodes.
    assert!(
        node_b_picture.entity(&EntityId("trk-1".into())).is_some(),
        "node B converged: trk-1 replicated from node A over PQ-mTLS"
    );
    assert!(
        node_a.picture().entity(&EntityId("trk-1".into())).is_some(),
        "node A still holds trk-1"
    );
    assert_eq!(node_a.picture().entity_count(), 1);
    assert_eq!(node_b_picture.entity_count(), 1);
}

#[test]
fn rc_26_unsigned_feed_is_rejected_at_the_boundary() {
    let idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let anchor = idp.trust_anchor();
    let (feed_cert, feed_ch_sk) = workload(&idp, "feed-src", &["feed"], 2);
    let (a_cert, a_ch_sk) = workload(&idp, "node-a", &["node"], 3);

    // Node A trusts feed "radar" → feed_vk, but the source will sign with a DIFFERENT
    // (impostor) key — the observation must be rejected, not fused.
    let (feed_vk, _feed_sk) = keypair(11);
    let (_impostor_vk, impostor_sk) = keypair(99);

    let feed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let feed_addr = feed_listener.local_addr().unwrap();

    let feed_pdp = permit_pdp(&["feed"]);
    let mut registry = FeedRegistry::new();
    registry.register(FeedId("radar".into()), feed_vk);
    let gateway = FeedGateway::new(registry, shared_fips());

    let feed_server = thread::spawn(move || {
        let (stream, _) = feed_listener.accept().unwrap();
        let provider = fips_provider();
        let mut rng = SeedRng(101);
        let responder = Responder::new(
            &provider,
            a_cert,
            &a_ch_sk,
            anchor.clone(),
            &feed_pdp,
            channel_resource(),
            &mut rng,
        );
        let mut admitted: Vec<VerifiedObservation> = Vec::new();
        let result = serve_feed_connection(stream, responder, &gateway, |v| admitted.push(v));
        (result.is_err(), admitted.len())
    });

    {
        let stream = TcpStream::connect(feed_addr).unwrap();
        let provider = fips_provider();
        let mut rng = SeedRng(102);
        let anchor2 = idp.trust_anchor();
        let init = Initiator::new(
            &provider,
            feed_cert,
            &feed_ch_sk,
            anchor2,
            pq_mtls::ServiceId("node-a".into()),
            Purpose("investigation".into()),
            posture(),
            &mut rng,
        );
        let mut client = FeedClient::connect(stream, init).expect("feed client connects");
        // Signed with the impostor key — the gateway trusts a different key for "radar".
        let signed = sign_observation(
            &FipsSig::new(ALG),
            &impostor_sk,
            radar_obs("trk-1", 100, 480.0),
        )
        .unwrap();
        client.send(&signed).expect("bytes sent");
    }

    let (was_rejected, admitted_count) = feed_server.join().unwrap();
    assert!(was_rejected, "an unsigned/wrong-key feed must be rejected");
    assert_eq!(
        admitted_count, 0,
        "nothing enters the picture from a bad feed"
    );
}
