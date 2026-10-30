//! Shared handshake test fixtures. They run on either backend: the deterministic test
//! backend, or the production FIPS backend (`--features backend-fips`), so the
//! authentication, downgrade and hardening tests are proven on the real ML-KEM,
//! X25519 and ML-DSA too. [MG-SEC-15]
#![allow(dead_code)]

use std::collections::BTreeSet;

use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicySet, Purpose, Resource,
    ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{
    certify, CertTerms, IdentityCert, Posture, ServiceId, TrustAnchor, WorkloadIdentity,
};
#[cfg(feature = "backend-fips")]
use pqc_agility::backend::fips::{provider as backend_provider, FipsSig as Sig};
#[cfg(feature = "test-backend")]
use pqc_agility::backend::test_backend::{provider as backend_provider, TestSig as Sig};
use pqc_agility::{CryptoProvider, Rng, SigId, SignatureScheme, SigningKey};

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

/// The signature algorithm used throughout the handshake tests.
pub const ALG: SigId = SigId::MlDsa65;

/// The selected backend's fully-populated crypto provider.
pub fn provider() -> CryptoProvider {
    backend_provider()
}

/// A certificate authority (trust anchor + its signing key).
pub struct Ca {
    pub anchor: TrustAnchor,
    pub signing_key: SigningKey,
}

/// Build a deterministic CA.
pub fn ca(seed: u64) -> Ca {
    let scheme = Sig::new(ALG);
    let (vk, sk) = scheme.keypair(&mut SeedRng::new(seed)).unwrap();
    Ca {
        anchor: TrustAnchor::new(ALG, vk),
        signing_key: sk,
    }
}

/// A workload party: cert, signing key, and identity.
pub struct Party {
    pub cert: IdentityCert,
    pub signing_key: SigningKey,
    pub identity: WorkloadIdentity,
}

/// Build a party whose identity is certified by `ca`.
pub fn party(id: &str, roles: &[&str], seed: u64, ca: &Ca) -> Party {
    let scheme = Sig::new(ALG);
    let (vk, sk) = scheme.keypair(&mut SeedRng::new(seed)).unwrap();
    let roles: BTreeSet<Role> = roles.iter().map(|r| Role(r.to_string())).collect();
    let identity = WorkloadIdentity {
        id: ServiceId(id.into()),
        roles,
        sig_alg: ALG,
        verifying_key: vk,
    };
    let cert = certify(&provider(), ALG, &ca.signing_key, &identity, terms(seed)).unwrap();
    Party {
        cert,
        signing_key: sk,
        identity,
    }
}

/// The terms test certificates are issued under: valid from the epoch to the end of
/// time, granting TOP SECRET, so the clearance a test requests is what it gets.
pub fn terms(serial: u64) -> CertTerms {
    CertTerms::new(
        serial,
        0,
        u64::MAX,
        Posture {
            level: ClassificationLevel::TOP_SECRET,
            compartments: BTreeSet::new(),
        },
    )
}

/// A channel resource with the given marking level and allowed purposes.
pub fn channel_resource(level: ClassificationLevel, purposes: &[&str]) -> Resource {
    Resource {
        id: ResourceId("channel".into()),
        marking: Marking {
            level,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: purposes.iter().map(|p| Purpose(p.to_string())).collect(),
    }
}

/// A PDP permitting `role` to `OpenChannel`.
pub fn permit_pdp(role: &str) -> DefaultPdp {
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

/// An empty PDP that denies everything (deny-by-default).
pub fn deny_pdp() -> DefaultPdp {
    DefaultPdp::new(PolicySet::new(vec![]).unwrap())
}
