//! The IdP issues PQ-signed tokens and workload certs entirely offline; every
//! verification failure is fail-closed. [NFR-SEC-2] [NFR-PRV-1]

use std::collections::BTreeSet;

use iam::{verify_token, Idp, TokenError, UserRecord};
use policy::{ClassificationLevel, Clearance, Purpose, Role};
use pq_mtls::{verify_cert, ServiceId, WorkloadIdentity};
use pqc_agility::Rng;

/// Deterministic RNG for reproducible keys in tests.
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

fn user(roles: &[&str], level: ClassificationLevel, purposes: &[&str]) -> UserRecord {
    UserRecord {
        roles: roles.iter().map(|r| Role(r.to_string())).collect(),
        clearance: Clearance {
            level,
            compartments: BTreeSet::new(),
        },
        purposes: purposes.iter().map(|p| Purpose(p.to_string())).collect(),
    }
}

fn idp_with_alice() -> Idp {
    let mut idp = Idp::generate("midgard-idp", &mut SeedRng(42)).unwrap();
    idp.store_mut().upsert(
        "alice",
        user(
            &["analyst"],
            ClassificationLevel::SECRET,
            &["investigation"],
        ),
    );
    idp
}

#[test]
fn issues_and_verifies_a_token_offline() {
    let idp = idp_with_alice();
    let now = 1_000_000;
    let token = idp
        .issue_token("alice", Purpose("investigation".into()), now, 3600)
        .expect("issue");

    let claims = verify_token(idp.provider(), idp.verifying_key(), &token, now + 10)
        .expect("verify within window");
    assert_eq!(claims.subject, "alice");
    assert_eq!(claims.issuer, "midgard-idp");
    assert!(claims.roles.contains(&Role("analyst".into())));
    assert_eq!(claims.level, ClassificationLevel::SECRET);
    assert_eq!(claims.purpose, Purpose("investigation".into()));

    // The claims project into a policy Subject for downstream authz.
    let subject = claims.to_subject();
    assert_eq!(subject.id.0, "alice");
}

#[test]
fn unknown_subject_and_unauthorised_purpose_are_denied() {
    let idp = idp_with_alice();
    assert!(idp
        .issue_token("bob", Purpose("investigation".into()), 1000, 60)
        .is_err());
    assert!(idp
        .issue_token("alice", Purpose("exfiltration".into()), 1000, 60)
        .is_err());
}

#[test]
fn tampered_token_fails_closed() {
    let idp = idp_with_alice();
    let now = 1_000_000;
    let mut token = idp
        .issue_token("alice", Purpose("investigation".into()), now, 3600)
        .unwrap();
    let n = token.len();
    token[n / 2] ^= 0x01;
    assert_eq!(
        verify_token(idp.provider(), idp.verifying_key(), &token, now + 10),
        Err(TokenError::BadSignature)
    );
}

#[test]
fn expired_and_not_yet_valid_fail_closed() {
    let idp = idp_with_alice();
    let now = 1_000_000;
    let token = idp
        .issue_token("alice", Purpose("investigation".into()), now, 3600)
        .unwrap();
    assert_eq!(
        verify_token(idp.provider(), idp.verifying_key(), &token, now - 1),
        Err(TokenError::NotYetValid)
    );
    assert_eq!(
        verify_token(idp.provider(), idp.verifying_key(), &token, now + 3600),
        Err(TokenError::Expired)
    );
}

#[test]
fn token_from_a_different_issuer_key_is_rejected() {
    let idp = idp_with_alice();
    let other = Idp::generate("evil-idp", &mut SeedRng(99)).unwrap();
    let now = 1_000_000;
    let token = idp
        .issue_token("alice", Purpose("investigation".into()), now, 3600)
        .unwrap();
    // Verified against the wrong issuer key → rejected.
    assert_eq!(
        verify_token(other.provider(), other.verifying_key(), &token, now + 10),
        Err(TokenError::BadSignature)
    );
}

#[test]
fn certifies_workload_identities_that_chain_to_the_anchor() {
    let idp = idp_with_alice();
    // A workload generates its own keypair; the IdP certifies its identity.
    let (wvk, _wsk) = idp
        .provider()
        .sig_for(iam::IDP_SIG_ALG)
        .unwrap()
        .keypair(&mut SeedRng(7))
        .unwrap();
    let identity = WorkloadIdentity {
        id: ServiceId("svc-forge".into()),
        roles: [Role("service".into())].into_iter().collect(),
        sig_alg: iam::IDP_SIG_ALG,
        verifying_key: wvk,
    };
    let cert = idp
        .certify(
            &identity,
            pq_mtls::CertTerms::new(
                1,
                0,
                u64::MAX,
                pq_mtls::Posture::at(policy::ClassificationLevel::TOP_SECRET),
            ),
        )
        .expect("certify");
    // The cert chains to the IdP trust anchor.
    verify_cert(&cert, &idp.trust_anchor(), idp.provider(), 1_000).expect("cert must chain");

    // A cert tampered to claim extra roles no longer verifies.
    let mut forged = cert.clone();
    forged.identity.roles.insert(Role("admin".into()));
    assert!(verify_cert(&forged, &idp.trust_anchor(), idp.provider(), 1_000).is_err());
}
