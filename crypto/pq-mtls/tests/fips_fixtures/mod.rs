//! Shared handshake fixtures over the production FIPS backend (backend-fips).
#![allow(dead_code)]

use std::collections::BTreeSet;

use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicySet, Purpose, Resource,
    ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{
    canonical_identity, IdentityCert, Posture, ServiceId, TrustAnchor, WorkloadIdentity,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{CryptoProvider, Rng, SigId, SigningKey, VerifyingKey};

/// Signature algorithm used for identities/CA (real ML-DSA-65).
pub const ALG: SigId = SigId::MlDsa65;

/// Deterministic splitmix64 RNG — stable across runs, exercises real keygen.
pub struct SeedRng(pub u64);
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

/// A fully-populated production provider.
pub fn provider() -> CryptoProvider {
    fips_provider()
}

fn keypair(p: &CryptoProvider, seed: u64) -> (VerifyingKey, SigningKey) {
    p.sig_for(ALG).unwrap().keypair(&mut SeedRng(seed)).unwrap()
}

/// A certificate authority (trust anchor + signing key).
pub struct Ca {
    pub anchor: TrustAnchor,
    pub signing_key: SigningKey,
}

/// Build a CA.
pub fn ca(p: &CryptoProvider, seed: u64) -> Ca {
    let (vk, sk) = keypair(p, seed);
    Ca {
        anchor: TrustAnchor::new(ALG, vk),
        signing_key: sk,
    }
}

/// A workload party: certified identity + its signing key.
pub struct Party {
    pub cert: IdentityCert,
    pub signing_key: SigningKey,
}

/// Build a party certified by `ca`.
pub fn party(p: &CryptoProvider, id: &str, roles: &[&str], seed: u64, ca: &Ca) -> Party {
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
        .sign(&ca.signing_key, &canonical_identity(&identity))
        .unwrap();
    Party {
        cert: IdentityCert {
            identity,
            ca_sig_alg: ALG,
            ca_signature,
        },
        signing_key: sk,
    }
}

/// An UNCLASSIFIED channel resource permitting the "investigation" purpose.
pub fn channel_resource() -> Resource {
    Resource {
        id: ResourceId("channel".into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

/// A PDP permitting `role` to OpenChannel.
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

/// A deny-by-default PDP (no rules).
pub fn deny_pdp() -> DefaultPdp {
    DefaultPdp::new(PolicySet::new(vec![]).unwrap())
}

/// A SECRET-cleared posture.
pub fn posture() -> Posture {
    Posture {
        level: ClassificationLevel::SECRET,
        compartments: BTreeSet::new(),
    }
}
