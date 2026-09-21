//! AC-H-2 — Given an unverified feed source, when it connects, then its data is
//! rejected from the picture. Exercises the PQ source-signature gate with the real
//! FIPS backend: only a correctly-signed observation from a registered feed becomes
//! a `VerifiedObservation`. [FR-H-3] [NFR-PQ-3]
#![cfg(feature = "backend-fips")]

use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::{shared, CryptoProvider};
use pqc_agility::{Rng, SharedProvider, SigId, SignatureScheme, VerifyingKey};

use heimdall_feed_gateway::{
    sign_observation, FeedError, FeedGateway, FeedRegistry, SignedObservation,
};
use heimdall_picture::{Attr, FeedId, Observation};

const ALG: SigId = SigId::MlDsa65;

/// A deterministic seeded RNG (SplitMix64) — test-only, matches the suite's pattern.
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

fn keypair(seed: u64) -> (VerifyingKey, pqc_agility::SigningKey) {
    FipsSig::new(ALG).keypair(&mut SeedRng(seed)).unwrap()
}

fn observation(feed: &str, key: &str) -> Observation {
    Observation::new("obs-1", FeedId(feed.into()), "air-track", key, 100)
        .with_attr("speed", Attr::Float(480.0))
        .with_attr("callsign", Attr::Text("VIPER1".into()))
}

fn sign(obs: Observation, sk: &pqc_agility::SigningKey) -> SignedObservation {
    sign_observation(&FipsSig::new(ALG), sk, obs).unwrap()
}

#[test]
fn ac_h_2_registered_feed_with_valid_signature_is_admitted() {
    let (vk, sk) = keypair(1);
    let mut registry = FeedRegistry::new();
    registry.register(FeedId("radar-1".into()), vk);
    let gateway = FeedGateway::new(registry, provider());

    let signed = sign(observation("radar-1", "trk-9"), &sk);
    let verified = gateway.authenticate(&signed).expect("valid feed admitted");
    assert_eq!(verified.observation().entity_key, "trk-9");
}

#[test]
fn ac_h_2_unregistered_feed_is_rejected() {
    let (vk, sk) = keypair(1);
    let mut registry = FeedRegistry::new();
    registry.register(FeedId("radar-1".into()), vk);
    let gateway = FeedGateway::new(registry, provider());

    // A source not in the trust registry — even with a real signature — is rejected.
    let signed = sign(observation("rogue-emitter", "trk-9"), &sk);
    assert_eq!(
        gateway.authenticate(&signed),
        Err(FeedError::UnknownFeed(FeedId("rogue-emitter".into())))
    );
}

#[test]
fn ac_h_2_tampered_observation_is_rejected() {
    let (vk, sk) = keypair(1);
    let mut registry = FeedRegistry::new();
    registry.register(FeedId("radar-1".into()), vk);
    let gateway = FeedGateway::new(registry, provider());

    // Sign a truthful observation, then tamper with the payload in flight.
    let mut signed = sign(observation("radar-1", "trk-9"), &sk);
    signed
        .observation
        .attrs
        .insert("speed".into(), Attr::Float(50.0));

    assert_eq!(
        gateway.authenticate(&signed),
        Err(FeedError::SignatureInvalid(FeedId("radar-1".into())))
    );
}

#[test]
fn ac_h_2_wrong_key_is_rejected() {
    // The feed is registered under key #1 but the observation is signed by key #2
    // (an impersonator holding a different key).
    let (vk1, _sk1) = keypair(1);
    let (_vk2, sk2) = keypair(2);
    let mut registry = FeedRegistry::new();
    registry.register(FeedId("radar-1".into()), vk1);
    let gateway = FeedGateway::new(registry, provider());

    let signed = sign(observation("radar-1", "trk-9"), &sk2);
    assert_eq!(
        gateway.authenticate(&signed),
        Err(FeedError::SignatureInvalid(FeedId("radar-1".into())))
    );
}
