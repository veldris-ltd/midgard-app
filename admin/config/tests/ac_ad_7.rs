//! AC-AD-7 — a configuration change is versioned with a diff and can be rolled
//! back; the history is PQ-signed and tamper-evident. `[FR-AD-7]` `[NFR-PQ-3]`

use admin_config::{ConfigError, ConfigState, ConfigStore};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::CryptoProvider;
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

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

fn keypair(seed: u64) -> (VerifyingKey, SigningKey) {
    FipsSig::new(ALG).keypair(&mut SeedRng(seed)).unwrap()
}

fn provider() -> CryptoProvider {
    let mut p = CryptoProvider::new();
    p.register_sig(Box::new(FipsSig::new(ALG)));
    p
}

fn store() -> ConfigStore {
    let (vk, sk) = keypair(3);
    ConfigStore::new(Box::new(FipsSig::new(ALG)), sk, vk)
}

#[test]
fn ac_ad_7_versioned_with_diff_and_rollback() {
    let mut store = store();

    // v0: initial config.
    let v0 = ConfigState::new()
        .with("marking.default", "UNCLASSIFIED")
        .with("session.ttl", "3600");
    assert_eq!(
        store.commit(v0, "alice", "initial config").unwrap().version,
        0
    );

    // v1: change a value and add a key.
    let v1 = ConfigState::new()
        .with("marking.default", "CONFIDENTIAL")
        .with("session.ttl", "3600")
        .with("mfa.required", "true");
    assert_eq!(
        store
            .commit(v1, "alice", "tighten defaults")
            .unwrap()
            .version,
        1
    );

    // Diff v0 → v1 shows exactly the change and the addition.
    let diff = store.diff(0, 1).unwrap();
    assert_eq!(
        diff.changed,
        vec![(
            "marking.default".to_string(),
            "UNCLASSIFIED".to_string(),
            "CONFIDENTIAL".to_string()
        )]
    );
    assert_eq!(
        diff.added,
        vec![("mfa.required".to_string(), "true".to_string())]
    );
    assert!(diff.removed.is_empty());

    // Roll back to v0 — an append-only forward commit restoring the old state.
    let rb = store.rollback(0, "bob", "revert tightening").unwrap();
    assert_eq!(rb.version, 2);
    assert_eq!(
        store.head().unwrap().state.get("marking.default").unwrap(),
        "UNCLASSIFIED"
    );
    assert!(store.head().unwrap().state.get("mfa.required").is_none());

    // History is intact (nothing rewritten) and v2 restored v0 exactly.
    assert_eq!(store.history().len(), 3);
    assert!(store.diff(0, 2).unwrap().is_empty());

    // The whole signed history verifies.
    assert!(store.verify(&provider()).is_ok());
}

#[test]
fn ac_ad_7_signature_verification_fails_closed_on_key_mismatch() {
    // A store that signs commits with key A but publishes key B as its verifying
    // key stands in for a tampered / substituted signature: verification must fail
    // closed at the first commit rather than trusting it.
    let (_vk_a, sk_a) = keypair(3);
    let (vk_b, _sk_b) = keypair(99);
    let mut store = ConfigStore::new(Box::new(FipsSig::new(ALG)), sk_a, vk_b);
    store
        .commit(ConfigState::new().with("k", "v0"), "alice", "c0")
        .unwrap();
    assert_eq!(
        store.verify(&provider()),
        Err(ConfigError::SignatureInvalid { version: 0 })
    );
}

#[test]
fn ac_ad_7_content_hash_binds_the_snapshot() {
    // The content hash is injective over the snapshot: two different states never
    // share a hash, so a post-signing state edit is detectable.
    let a = ConfigState::new().with("k", "v0");
    let b = ConfigState::new().with("k", "v1");
    assert_ne!(a.content_hash(), b.content_hash());
}

#[test]
fn ac_ad_7_empty_author_is_rejected() {
    let mut store = store();
    assert!(store.commit(ConfigState::new(), "", "no author").is_err());
}
