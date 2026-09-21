//! AC-C-9 — biometric / WebAuthn authentication where no secret leaves the secure
//! boundary. The authenticator answers a challenge inside custody; only an
//! assertion crosses the API, and the gate verifies it against the public credential.
#![cfg(feature = "backend-fips")]

use midgard_client_keystore::{
    verify_assertion, AuthChallenge, AuthMechanism, PlatformAuthenticator, PlatformProfile,
    SoftwareAuthenticator,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, SigId};

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, dest: &mut [u8]) {
        for b in dest {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (self.0 >> 33) as u8;
        }
    }
}

#[test]
fn a_valid_biometric_assertion_verifies() {
    let provider = fips_provider();
    let auth = SoftwareAuthenticator::enrol(
        &provider,
        "device-credential",
        AuthMechanism::FaceId,
        SigId::MlDsa65,
        &mut SeedRng(1),
    )
    .unwrap();

    let challenge = AuthChallenge::sample(&mut SeedRng(2));
    let assertion = auth.authenticate(&provider, &challenge).unwrap();

    // The gate accepts a genuine assertion against the public credential.
    verify_assertion(auth.credential(), &challenge, &assertion, &provider).unwrap();
    assert_eq!(assertion.mechanism, AuthMechanism::FaceId);
}

#[test]
fn a_replayed_or_tampered_challenge_is_denied() {
    let provider = fips_provider();
    let auth = SoftwareAuthenticator::enrol(
        &provider,
        "device-credential",
        AuthMechanism::WindowsHello,
        SigId::MlDsa65,
        &mut SeedRng(3),
    )
    .unwrap();

    let challenge = AuthChallenge::sample(&mut SeedRng(4));
    let assertion = auth.authenticate(&provider, &challenge).unwrap();

    // An assertion for one challenge must not verify against a different one.
    let other = AuthChallenge::sample(&mut SeedRng(5));
    assert!(verify_assertion(auth.credential(), &other, &assertion, &provider).is_err());
}

#[test]
fn a_foreign_credential_is_denied() {
    let provider = fips_provider();
    let real = SoftwareAuthenticator::enrol(
        &provider,
        "device-credential",
        AuthMechanism::TouchId,
        SigId::MlDsa65,
        &mut SeedRng(6),
    )
    .unwrap();
    let attacker = SoftwareAuthenticator::enrol(
        &provider,
        "attacker-credential",
        AuthMechanism::TouchId,
        SigId::MlDsa65,
        &mut SeedRng(7),
    )
    .unwrap();

    let challenge = AuthChallenge::sample(&mut SeedRng(8));
    // The attacker signs, but presents against the real credential → denied.
    let forged = attacker.authenticate(&provider, &challenge).unwrap();
    assert!(verify_assertion(real.credential(), &challenge, &forged, &provider).is_err());
}

#[test]
fn assertion_carries_no_secret_key_material() {
    // Structural AC-C-9: the assertion type exposes only a credential id, mechanism,
    // and a signature — there is no field or method yielding the credential secret.
    let provider = fips_provider();
    let auth = SoftwareAuthenticator::enrol(
        &provider,
        "device-credential",
        AuthMechanism::WebAuthn,
        SigId::MlDsa65,
        &mut SeedRng(9),
    )
    .unwrap();
    let challenge = AuthChallenge::sample(&mut SeedRng(10));
    let assertion = auth.authenticate(&provider, &challenge).unwrap();
    // The only bytes exposed are the signature over the public challenge.
    assert_eq!(assertion.credential_id, "device-credential");
    assert!(!assertion.signature.as_bytes().is_empty());
    // The public credential exposes a verifying key, never a signing key.
    let _vk = &auth.credential().public_key;
}

#[test]
fn mechanism_defaults_match_each_platform() {
    assert_eq!(
        AuthMechanism::for_platform(PlatformProfile::Apple),
        AuthMechanism::FaceId
    );
    assert_eq!(
        AuthMechanism::for_platform(PlatformProfile::Android),
        AuthMechanism::AndroidBiometricPrompt
    );
    assert_eq!(
        AuthMechanism::for_platform(PlatformProfile::Windows),
        AuthMechanism::WindowsHello
    );
    assert_eq!(
        AuthMechanism::for_platform(PlatformProfile::WebApp),
        AuthMechanism::WebAuthn
    );
}
