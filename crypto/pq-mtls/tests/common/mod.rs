//! Shared handshake test fixtures (test backend only).
#![allow(dead_code)]

use std::collections::BTreeSet;

use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicySet, Purpose, Resource,
    ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{
    certify, CertTerms, IdentityCert, Posture, ServiceId, TrustAnchor, WorkloadIdentity,
};
use pqc_agility::backend::test_backend::{provider as test_provider, TestSig};
use pqc_agility::{CryptoProvider, SeedRng, SigId, SignatureScheme, SigningKey};

/// The signature algorithm used throughout the handshake tests.
pub const ALG: SigId = SigId::MlDsa65;

/// A fully-populated test crypto provider.
pub fn provider() -> CryptoProvider {
    test_provider()
}

/// A certificate authority (trust anchor + its signing key).
pub struct Ca {
    pub anchor: TrustAnchor,
    pub signing_key: SigningKey,
}

/// Build a deterministic CA.
pub fn ca(seed: u64) -> Ca {
    let scheme = TestSig::new(ALG);
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
    let scheme = TestSig::new(ALG);
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
