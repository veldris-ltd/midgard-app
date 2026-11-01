//! Shared handshake fixtures for the enclosure-client tests (test backend only).
//! [AC-C-3]
#![allow(dead_code)]

use std::collections::BTreeSet;

use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicySet, Purpose, Resource,
    ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{IdentityCert, ServiceId, TrustAnchor, WorkloadIdentity};
use pqc_agility::backend::test_backend::{provider as test_provider, TestSig};
use pqc_agility::{CryptoProvider, KemSuiteId, SeedRng, SigId, SignatureScheme, SigningKey};

/// The signature algorithm used throughout the tests.
pub const ALG: SigId = SigId::MlDsa65;

/// A fully-populated test crypto provider.
pub fn provider() -> CryptoProvider {
    test_provider()
}

/// A provider that offers no KEM suites at all — models a stripped / classical-only
/// client whose PQ negotiation must fail closed.
pub fn provider_without_kems() -> CryptoProvider {
    let mut p = test_provider();
    for s in [
        KemSuiteId::MlKem768,
        KemSuiteId::MlKem1024,
        KemSuiteId::HybridX25519MlKem768,
        KemSuiteId::HybridX25519MlKem1024,
    ] {
        p.disable_kem(s);
    }
    p
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

/// Build a party certified by `ca`.
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
    let ca_scheme = TestSig::new(ALG);
    let ca_sig = ca_scheme
        .sign(
            &ca.signing_key,
            &pq_mtls::canonical_cert_body(&identity, &test_terms()),
        )
        .unwrap();
    let cert = IdentityCert {
        identity: identity.clone(),
        terms: test_terms(),
        ca_sig_alg: ALG,
        ca_signature: ca_sig,
    };
    Party {
        cert,
        signing_key: sk,
        identity,
    }
}

/// The enclosure channel resource (unclassified, purpose "investigation").
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

/// A PDP permitting `role` to open a channel.
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
