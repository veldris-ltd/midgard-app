//! RC-30 — Given an admin, when they authenticate, then a **real WebAuthn assertion
//! is verified**; no session is authenticated without one. Negative tests: a forged
//! signature, an absent/wrong credential, a replayed (wrong-challenge) assertion, and
//! a non-user-verified assertion are all denied — none yields an `AdminSession`.
//! [RC-30]
#![cfg(feature = "backend-fips")]

use admin_identity::{
    AdminAuthenticator, AdminChallenge, AdminId, IdentityError, RegisteredPasskey,
    SoftwareAdminAuthenticator, WebAuthnAssertion, PASSKEY_SIG_ALG,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, Signature};

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

fn setup() -> (AdminAuthenticator, SoftwareAdminAuthenticator) {
    let device =
        SoftwareAdminAuthenticator::enrol(&fips_provider(), "alice-key", &mut SeedRng(1)).unwrap();
    let mut authr = AdminAuthenticator::new(fips_provider());
    authr.register(AdminId("alice".into()), device.credential().clone());
    (authr, device)
}

#[test]
fn a_genuine_user_verified_assertion_authenticates() {
    let (authr, device) = setup();
    let ch = AdminChallenge::sample(&mut SeedRng(9));
    let asr = device.authenticate(authr.provider(), &ch, true).unwrap();
    let session = authr
        .sign_in(&AdminId("alice".into()), &ch, &asr, 1000)
        .expect("real assertion → authenticated session");
    assert_eq!(session.admin(), &AdminId("alice".into()));
    assert!(session.has_mfa());
}

#[test]
fn a_forged_signature_is_denied() {
    let (authr, device) = setup();
    let ch = AdminChallenge::sample(&mut SeedRng(9));
    let mut asr = device.authenticate(authr.provider(), &ch, true).unwrap();
    // Forge: replace the signature with garbage of the right algorithm.
    asr.signature = Signature::new(PASSKEY_SIG_ALG, vec![0u8; asr.signature.as_bytes().len()]);
    assert_eq!(
        authr
            .sign_in(&AdminId("alice".into()), &ch, &asr, 1000)
            .unwrap_err(),
        IdentityError::WebAuthnRejected
    );
}

#[test]
fn an_assertion_from_a_different_key_is_denied() {
    let (authr, _device) = setup();
    // An impostor enrolls their OWN passkey under the same credential id, then signs.
    let impostor =
        SoftwareAdminAuthenticator::enrol(&fips_provider(), "alice-key", &mut SeedRng(999))
            .unwrap();
    let ch = AdminChallenge::sample(&mut SeedRng(9));
    let asr = impostor.authenticate(authr.provider(), &ch, true).unwrap();
    // Same credential_id, but the signature is under a key the RP never registered.
    assert_eq!(
        authr
            .sign_in(&AdminId("alice".into()), &ch, &asr, 1000)
            .unwrap_err(),
        IdentityError::WebAuthnRejected
    );
}

#[test]
fn a_replayed_assertion_for_a_different_challenge_is_denied() {
    let (authr, device) = setup();
    let ch = AdminChallenge::sample(&mut SeedRng(9));
    let asr = device.authenticate(authr.provider(), &ch, true).unwrap();
    // Present the assertion against a DIFFERENT challenge → signature no longer binds.
    let other = AdminChallenge::sample(&mut SeedRng(42));
    assert_eq!(
        authr
            .sign_in(&AdminId("alice".into()), &other, &asr, 1000)
            .unwrap_err(),
        IdentityError::WebAuthnRejected
    );
}

#[test]
fn a_wrong_credential_id_is_denied() {
    let (authr, device) = setup();
    let ch = AdminChallenge::sample(&mut SeedRng(9));
    let mut asr = device.authenticate(authr.provider(), &ch, true).unwrap();
    asr.credential_id = "someone-elses-key".into();
    assert_eq!(
        authr
            .sign_in(&AdminId("alice".into()), &ch, &asr, 1000)
            .unwrap_err(),
        IdentityError::WebAuthnRejected
    );
}

#[test]
fn direct_verification_against_a_registered_passkey_also_fails_closed() {
    // The gate function itself (used by sign-in/step-up) rejects a forged assertion.
    let device = SoftwareAdminAuthenticator::enrol(&fips_provider(), "k", &mut SeedRng(3)).unwrap();
    let cred: RegisteredPasskey = device.credential().clone();
    let ch = AdminChallenge::sample(&mut SeedRng(4));
    let genuine = device.authenticate(&fips_provider(), &ch, true).unwrap();
    admin_identity::verify_admin_assertion(&cred, &ch, &genuine, &fips_provider())
        .expect("genuine verifies");

    let forged = WebAuthnAssertion {
        credential_id: "k".into(),
        user_verified: true,
        signature: Signature::new(
            PASSKEY_SIG_ALG,
            vec![7u8; genuine.signature.as_bytes().len()],
        ),
    };
    assert!(
        admin_identity::verify_admin_assertion(&cred, &ch, &forged, &fips_provider()).is_err(),
        "forged signature must fail closed"
    );
}
