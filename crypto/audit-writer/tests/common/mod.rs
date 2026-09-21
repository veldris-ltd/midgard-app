//! Shared helpers for audit-writer integration tests (test backend only).
#![allow(dead_code)]

use audit_writer::{Action, Actor, AuditRecord, RecordId, Resource, Timestamp};
use pqc_agility::backend::test_backend::TestSig;
use pqc_agility::{SeedRng, SigId, SignatureScheme, SigningKey, VerifyingKey};

/// Build a simple audit record.
pub fn record(id: &str, actor: &str, action: &str, resource: &str) -> AuditRecord {
    AuditRecord {
        id: RecordId(id.into()),
        actor: Actor(actor.into()),
        action: Action(action.into()),
        resource: Resource(resource.into()),
        ts: Timestamp("2026-07-08T00:00:00Z".into()),
    }
}

/// Deterministically derive a signer: a boxed scheme plus its keypair.
pub fn signer(alg: SigId, seed: u64) -> (Box<dyn SignatureScheme>, SigningKey, VerifyingKey) {
    let scheme = TestSig::new(alg);
    let mut rng = SeedRng::new(seed);
    let (vk, sk) = scheme.keypair(&mut rng).unwrap();
    (Box::new(TestSig::new(alg)), sk, vk)
}

/// Reconstruct the same signing key produced by [`signer`] (same seed).
pub fn signing_key(alg: SigId, seed: u64) -> SigningKey {
    let scheme = TestSig::new(alg);
    let mut rng = SeedRng::new(seed);
    let (_vk, sk) = scheme.keypair(&mut rng).unwrap();
    sk
}
