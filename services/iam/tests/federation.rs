//! Federation assertions are scoped and single-use: the bridge refuses an assertion
//! that is expired or not yet valid, replayed, for another audience or from another
//! issuer, overlong, or in the earlier unversioned shape, and mints a local token only
//! for a fresh, correctly scoped one. [RC-29] [AC-17.1] [AC-17.2] [AC-17.3] [AC-17.4]
//! [AC-17.5] [VLD-ANB-MIDGARD-0049]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;

use iam::federation::MAX_ASSERTION_LIFETIME;
use iam::{
    federated_login, verify_token, FederatedClaims, FederatedIdp, FederationError, Idp,
    PqFederation, UserRecord,
};
use policy::{ClassificationLevel, Clearance, Purpose, Role};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, SigId, SigningKey};

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

const NOW: u64 = 1_800_000_000;
const TTL: u64 = 3600;
const ISSUER: &str = "upstream-corp";
const AUDIENCE: &str = "midgard-idp";

fn analyst_user() -> UserRecord {
    UserRecord {
        roles: [Role("analyst".into())].into_iter().collect(),
        clearance: Clearance {
            level: ClassificationLevel::SECRET,
            compartments: BTreeSet::new(),
        },
        purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

/// A trusted upstream (its signing key) and a bridge configured to trust it for
/// `ISSUER` and `AUDIENCE`.
fn upstream_and_bridge() -> (SigningKey, PqFederation) {
    let (up_vk, up_sk) = fips_provider()
        .sig_for(SigId::MlDsa65)
        .unwrap()
        .keypair(&mut SeedRng(50))
        .unwrap();
    let bridge = PqFederation::new(fips_provider(), up_vk, SigId::MlDsa65, ISSUER, AUDIENCE);
    (up_sk, bridge)
}

/// Fresh, correctly scoped claims issued at `NOW` and valid for two minutes.
fn claims(nonce: u8) -> FederatedClaims {
    FederatedClaims {
        issuer: ISSUER.into(),
        audience: AUDIENCE.into(),
        issued_at: NOW,
        expires_at: NOW + 120,
        nonce: [nonce; 16],
        subject: "carol@partner.example".into(),
        user: analyst_user(),
    }
}

/// The upstream signs `claims` and encodes the wire assertion.
fn sign(up_sk: &SigningKey, claims: &FederatedClaims) -> Vec<u8> {
    let payload = PqFederation::assertion_payload(claims);
    let sig = fips_provider()
        .sig_for(SigId::MlDsa65)
        .unwrap()
        .sign(up_sk, &payload)
        .unwrap();
    PqFederation::encode_assertion(claims, &sig)
}

fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

/// A test-local copy of the earlier (v1) payload: `subject ‖ roles ‖ level ‖
/// compartments ‖ purposes`, with no version byte, scope, validity or nonce.
fn v1_payload(claims: &FederatedClaims) -> Vec<u8> {
    let mut o = Vec::new();
    put_bytes(&mut o, claims.subject.as_bytes());
    o.extend_from_slice(&(claims.user.roles.len() as u32).to_le_bytes());
    for r in &claims.user.roles {
        put_bytes(&mut o, r.0.as_bytes());
    }
    o.push(claims.user.clearance.level.0);
    o.extend_from_slice(&(claims.user.clearance.compartments.len() as u32).to_le_bytes());
    for c in &claims.user.clearance.compartments {
        put_bytes(&mut o, c.0.as_bytes());
    }
    o.extend_from_slice(&(claims.user.purposes.len() as u32).to_le_bytes());
    for p in &claims.user.purposes {
        put_bytes(&mut o, p.0.as_bytes());
    }
    o
}

/// The wire framing around an arbitrary payload, as `encode_assertion` lays it out.
fn frame(payload: &[u8], up_sk: &SigningKey) -> Vec<u8> {
    let sig = fips_provider()
        .sig_for(SigId::MlDsa65)
        .unwrap()
        .sign(up_sk, payload)
        .unwrap();
    let mut o = Vec::new();
    o.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    o.extend_from_slice(payload);
    o.push(sig.alg().as_u8());
    o.extend_from_slice(&(sig.as_bytes().len() as u32).to_le_bytes());
    o.extend_from_slice(sig.as_bytes());
    o
}

// [AC-17.1]
#[test]
fn expired_and_future_assertions_are_refused() {
    let (up_sk, mut bridge) = upstream_and_bridge();
    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();

    let mut expired = claims(1);
    expired.issued_at = NOW - 100;
    expired.expires_at = NOW - 1;
    let assertion = sign(&up_sk, &expired);
    assert_eq!(
        bridge.verify(&assertion, NOW),
        Err(FederationError::Expired)
    );
    // Expiry is exclusive: at exactly expires_at the assertion is already stale.
    let mut at_edge = claims(2);
    at_edge.expires_at = NOW;
    at_edge.issued_at = NOW - 60;
    assert_eq!(
        bridge.verify(&sign(&up_sk, &at_edge), NOW),
        Err(FederationError::Expired)
    );

    let mut future = claims(3);
    future.issued_at = NOW + 120;
    future.expires_at = NOW + 240;
    let assertion = sign(&up_sk, &future);
    assert_eq!(
        bridge.verify(&assertion, NOW),
        Err(FederationError::NotYetValid)
    );
    // No token is minted for either.
    assert_eq!(
        federated_login(
            &idp,
            &mut bridge,
            &assertion,
            Purpose("investigation".into()),
            NOW,
            TTL
        ),
        Err(FederationError::NotYetValid)
    );

    // Within the clock skew an early issued_at is accepted.
    let mut skewed = claims(4);
    skewed.issued_at = NOW + 60;
    skewed.expires_at = NOW + 180;
    assert!(bridge.verify(&sign(&up_sk, &skewed), NOW).is_ok());
    assert_eq!(bridge.seen_len(), 1, "only the accepted nonce is recorded");
}

// [AC-17.2]
#[test]
fn a_replayed_assertion_is_refused_and_the_cache_is_bounded() {
    let (up_sk, mut bridge) = upstream_and_bridge();

    let first = sign(&up_sk, &claims(1));
    assert!(bridge.verify(&first, NOW).is_ok());
    assert_eq!(bridge.seen_len(), 1);

    // The identical bytes again, inside their validity: a replay.
    assert_eq!(
        bridge.verify(&first, NOW + 10),
        Err(FederationError::Replayed)
    );
    // A different assertion reusing the nonce is a replay too.
    let mut same_nonce = claims(1);
    same_nonce.issued_at = NOW + 5;
    same_nonce.expires_at = NOW + 100;
    assert_eq!(
        bridge.verify(&sign(&up_sk, &same_nonce), NOW + 10),
        Err(FederationError::Replayed)
    );
    assert_eq!(bridge.seen_len(), 1);

    // After expiry the nonce is forgotten on the next verify, whatever its outcome:
    // the stale replay is refused as expired and the cache is empty again.
    let later = NOW + MAX_ASSERTION_LIFETIME + 1;
    assert_eq!(bridge.verify(&first, later), Err(FederationError::Expired));
    assert_eq!(bridge.seen_len(), 0);

    // A new assertion at that time is accepted and the cache holds only it.
    let mut fresh = claims(2);
    fresh.issued_at = later;
    fresh.expires_at = later + 120;
    assert!(bridge.verify(&sign(&up_sk, &fresh), later).is_ok());
    assert_eq!(bridge.seen_len(), 1);
}

// [AC-17.3]
#[test]
fn audience_and_issuer_mismatches_are_refused() {
    let (up_sk, mut bridge) = upstream_and_bridge();

    let mut other_audience = claims(1);
    other_audience.audience = "other-enclosure".into();
    assert_eq!(
        bridge.verify(&sign(&up_sk, &other_audience), NOW),
        Err(FederationError::WrongAudience)
    );

    let mut other_issuer = claims(2);
    other_issuer.issuer = "someone-else".into();
    assert_eq!(
        bridge.verify(&sign(&up_sk, &other_issuer), NOW),
        Err(FederationError::WrongIssuer)
    );
    assert_eq!(bridge.seen_len(), 0, "a refused assertion spends no nonce");
}

// [AC-17.4]
#[test]
fn overlong_lifetime_and_v1_payloads_are_refused() {
    let (up_sk, mut bridge) = upstream_and_bridge();

    let mut overlong = claims(1);
    overlong.expires_at = NOW + MAX_ASSERTION_LIFETIME + 1;
    assert_eq!(
        bridge.verify(&sign(&up_sk, &overlong), NOW),
        Err(FederationError::LifetimeTooLong)
    );
    // An empty or inverted window is refused the same way.
    let mut inverted = claims(2);
    inverted.expires_at = NOW - 10;
    assert_eq!(
        bridge.verify(&sign(&up_sk, &inverted), NOW),
        Err(FederationError::LifetimeTooLong)
    );
    // Exactly the cap is accepted.
    let mut at_cap = claims(3);
    at_cap.expires_at = NOW + MAX_ASSERTION_LIFETIME;
    assert!(bridge.verify(&sign(&up_sk, &at_cap), NOW).is_ok());

    // A v1-shaped payload, correctly signed by the upstream, is not misparsed.
    let v1 = frame(&v1_payload(&claims(4)), &up_sk);
    assert_eq!(bridge.verify(&v1, NOW), Err(FederationError::Malformed));
}

// [AC-17.5]
#[test]
fn a_fresh_assertion_mints_a_local_token() {
    let (up_sk, mut bridge) = upstream_and_bridge();
    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();

    let assertion = sign(&up_sk, &claims(1));
    let token = federated_login(
        &idp,
        &mut bridge,
        &assertion,
        Purpose("investigation".into()),
        NOW,
        TTL,
    )
    .expect("a fresh, correctly scoped assertion logs in");
    let out = verify_token(idp.provider(), idp.verifying_key(), &token, NOW).unwrap();
    assert_eq!(out.subject, "carol@partner.example");
    assert!(out.roles.contains(&Role("analyst".into())));

    // The same assertion cannot log in twice.
    assert_eq!(
        federated_login(
            &idp,
            &mut bridge,
            &assertion,
            Purpose("investigation".into()),
            NOW,
            TTL
        ),
        Err(FederationError::Replayed)
    );
}
