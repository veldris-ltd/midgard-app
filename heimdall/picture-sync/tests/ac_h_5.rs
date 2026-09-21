//! AC-H-5 — Given a multi-node deployment, when the picture updates, then it
//! replicates across nodes over PQ-mTLS. Two nodes build divergent local pictures,
//! exchange PQ-signed deltas over the transport seam, verify each other's
//! signatures, and converge. An unverified (tampered or wrong-key) delta is
//! rejected and mutates nothing. [FR-H-6] [NFR-PQ-3]
#![cfg(feature = "backend-fips")]

use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::{shared, CryptoProvider};
use pqc_agility::{Rng, SharedProvider, SigId, SignatureScheme, SigningKey, VerifyingKey};

use heimdall_feed_gateway::{sign_observation, FeedGateway, FeedRegistry, VerifiedObservation};
use heimdall_fusion::FusionEngine;
use heimdall_picture::{Attr, EntityId, FeedId, Observation};
use heimdall_picture_sync::{loopback_link, DeltaLink, Replicator, SyncError};

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

fn provider() -> SharedProvider {
    let mut p = CryptoProvider::new();
    p.register_sig(Box::new(FipsSig::new(ALG)));
    shared(p)
}

fn keypair(seed: u64) -> (VerifyingKey, SigningKey) {
    FipsSig::new(ALG).keypair(&mut SeedRng(seed)).unwrap()
}

/// A gateway trusting one feed, plus that feed's signer.
fn feed_gateway() -> (FeedGateway, SigningKey) {
    let (vk, sk) = keypair(1);
    let mut registry = FeedRegistry::new();
    registry.register(FeedId("radar".into()), vk);
    (FeedGateway::new(registry, provider()), sk)
}

fn verified(
    gateway: &FeedGateway,
    sk: &SigningKey,
    key: &str,
    t: u64,
    speed: f64,
) -> VerifiedObservation {
    let obs = Observation::new(
        format!("radar-{key}-{t}"),
        FeedId("radar".into()),
        "air-track",
        key,
        t,
    )
    .with_attr("speed", Attr::Float(speed));
    gateway
        .authenticate(&sign_observation(&FipsSig::new(ALG), sk, obs).unwrap())
        .expect("authenticates")
}

#[test]
fn ac_h_5_two_nodes_converge_over_signed_deltas() {
    let (gw, feed_sk) = feed_gateway();

    // Node A sees A123 (t=100) and A124 (t=101).
    let mut node_a = FusionEngine::new();
    node_a.ingest(&verified(&gw, &feed_sk, "A123", 100, 480.0));
    node_a.ingest(&verified(&gw, &feed_sk, "A124", 101, 100.0));

    // Node B sees S9 (t=102) and an OLDER A123 (t=90) — the merge tie-break must
    // keep node A's newer A123.
    let mut node_b = FusionEngine::new();
    node_b.ingest(&verified(&gw, &feed_sk, "S9", 102, 12.0));
    node_b.ingest(&verified(&gw, &feed_sk, "A123", 90, 300.0));

    let (a_vk, a_sk) = keypair(10);
    let (b_vk, b_sk) = keypair(20);
    let repl = Replicator::new(provider());

    // Exchange deltas over the PQ transport seam (loopback stands in for pq-mtls).
    let (mut link_a, mut link_b) = loopback_link();
    link_a
        .send(repl.export(&a_sk, node_a.picture()).unwrap())
        .unwrap();
    link_b
        .send(repl.export(&b_sk, node_b.picture()).unwrap())
        .unwrap();

    // Node B applies A's delta (verified against A's key); Node A applies B's.
    let from_a = link_b.recv().unwrap();
    repl.import(&a_vk, &from_a, node_b.picture_mut()).unwrap();
    let from_b = link_a.recv().unwrap();
    repl.import(&b_vk, &from_b, node_a.picture_mut()).unwrap();

    // Both nodes now hold the union {A123, A124, S9}.
    assert_eq!(node_a.picture().entity_count(), 3);
    assert_eq!(node_b.picture().entity_count(), 3);

    // Node B's older A123 was overwritten by node A's newer one (last-write-wins).
    let b_a123 = node_b.picture().entity(&EntityId("A123".into())).unwrap();
    assert_eq!(b_a123.track.last_seen, 100);
    // Node A gained S9 from node B.
    assert!(node_a.picture().entity(&EntityId("S9".into())).is_some());
}

#[test]
fn ac_h_5_tampered_delta_is_rejected_and_merges_nothing() {
    let (gw, feed_sk) = feed_gateway();
    let mut node_a = FusionEngine::new();
    node_a.ingest(&verified(&gw, &feed_sk, "A123", 100, 480.0));

    let (a_vk, a_sk) = keypair(10);
    let repl = Replicator::new(provider());

    let mut delta = repl.export(&a_sk, node_a.picture()).unwrap();
    // Flip a byte in the signed payload.
    let last = delta.bytes.len() - 1;
    delta.bytes[last] ^= 0xFF;

    let mut node_b = FusionEngine::new();
    assert_eq!(
        repl.import(&a_vk, &delta, node_b.picture_mut()),
        Err(SyncError::SignatureInvalid)
    );
    assert_eq!(node_b.picture().entity_count(), 0);
}

#[test]
fn ac_h_5_delta_from_untrusted_node_is_rejected() {
    let (gw, feed_sk) = feed_gateway();
    let mut node_a = FusionEngine::new();
    node_a.ingest(&verified(&gw, &feed_sk, "A123", 100, 480.0));

    let (_a_vk, a_sk) = keypair(10);
    let (impostor_vk, _impostor_sk) = keypair(99);
    let repl = Replicator::new(provider());

    // A's delta, but verified against an unrelated node's key → rejected.
    let delta = repl.export(&a_sk, node_a.picture()).unwrap();
    let mut node_b = FusionEngine::new();
    assert_eq!(
        repl.import(&impostor_vk, &delta, node_b.picture_mut()),
        Err(SyncError::SignatureInvalid)
    );
    assert_eq!(node_b.picture().entity_count(), 0);
}
