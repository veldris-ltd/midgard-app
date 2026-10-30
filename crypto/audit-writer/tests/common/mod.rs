//! Shared helpers for audit-writer integration tests. They run on either backend: the
//! deterministic test backend, or the production FIPS backend (`--features
//! backend-fips`), so tamper and truncation detection is proven on the real ML-DSA
//! too. [MG-SEC-15]
#![allow(dead_code)]

use audit_writer::{Action, Actor, AuditRecord, RecordId, Resource, Timestamp};
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

#[cfg(feature = "backend-fips")]
pub use pqc_agility::backend::fips::{provider, FipsSig as Sig};
#[cfg(feature = "test-backend")]
pub use pqc_agility::backend::test_backend::{provider, TestSig as Sig};

/// Deterministic splitmix64 RNG, so a seed gives the same key on either backend.
pub struct SeedRng(u64);
impl SeedRng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
}
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            chunk.copy_from_slice(&z.to_le_bytes()[..chunk.len()]);
        }
    }
}

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
    let scheme = Sig::new(alg);
    let mut rng = SeedRng::new(seed);
    let (vk, sk) = scheme.keypair(&mut rng).unwrap();
    (Box::new(Sig::new(alg)), sk, vk)
}

/// Reconstruct the same signing key produced by [`signer`] (same seed).
pub fn signing_key(alg: SigId, seed: u64) -> SigningKey {
    let scheme = Sig::new(alg);
    let mut rng = SeedRng::new(seed);
    let (_vk, sk) = scheme.keypair(&mut rng).unwrap();
    sk
}
