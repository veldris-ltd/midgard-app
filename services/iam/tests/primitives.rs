//! Unit coverage for the credential primitives: password hashing (Argon2id, and
//! legacy PBKDF2 within its cost bounds), TOTP, and WebAuthn assertion verification.
//! [RC-29] [NFR-SEC-2] [MG-SEC-11] [VLD-ANB-MIDGARD-0025]
#![cfg(feature = "backend-fips")]

use iam::mfa::{AuthAssertion, WebAuthnCredential, PASSKEY_SIG_ALG};
use std::time::{Duration, Instant};

use iam::{Argon2idHasher, PasswordHasher, Pbkdf2Hasher, TotpSecret, MIN_PBKDF2_ITERATIONS};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

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

#[test]
fn pbkdf2_hash_verifies_and_rejects_wrong_password() {
    let h = Pbkdf2Hasher::new(2000);
    let enc = h.hash(b"s3cret-pass");
    assert!(enc.starts_with("pbkdf2-sha256$i=2000$"));
    assert!(h.verify(b"s3cret-pass", &enc));
    assert!(!h.verify(b"s3cret-Pass", &enc));
    assert!(!h.verify(b"", &enc));
    // Two hashes of the same password differ (random salt).
    assert_ne!(enc, h.hash(b"s3cret-pass"));
    // A malformed encoding fails closed.
    assert!(!h.verify(b"s3cret-pass", "not-a-hash"));
}

/// AC-56.1: the production hasher writes Argon2id at the OWASP minimum, with a fresh
/// salt each time, and fails closed on a wrong password or a malformed entry.
#[test]
fn argon2id_hashes_verify_and_fail_closed() {
    let h = Argon2idHasher::production();
    let enc = h.hash(b"s3cret-pass");
    assert!(enc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{enc}");
    assert!(h.verify(b"s3cret-pass", &enc));
    assert!(!h.verify(b"s3cret-Pass", &enc));
    assert!(!h.verify(b"", &enc));
    assert_ne!(enc, h.hash(b"s3cret-pass"), "two hashes share a salt");
    for bad in ["not-a-hash", "$argon2id$", &enc[..enc.len() - 4]] {
        assert!(!h.verify(b"s3cret-pass", bad), "{bad:?}");
    }
}

/// AC-56.3: a stored PBKDF2 iteration count outside the bounds is refused before any
/// work is done, whichever way it was tampered with; the legacy cost still verifies;
/// and the debug-build cost knob holds its own count as the floor.
#[test]
fn pbkdf2_iteration_floor_and_ceiling_are_enforced() {
    let production = Pbkdf2Hasher::production();
    // Inert literals: an all-zero 16-byte salt and 32-byte hash.
    let zeros = |n: usize| "0".repeat(n * 2);
    for iterations in [1u32, u32::MAX] {
        let tampered = format!("pbkdf2-sha256$i={iterations}${}${}", zeros(16), zeros(32));
        let started = Instant::now();
        assert!(
            !production.verify(b"s3cret-pass", &tampered),
            "i={iterations}"
        );
        let took = started.elapsed();
        assert!(
            took < Duration::from_millis(50),
            "i={iterations} took {took:?}: the bound must be checked before the work"
        );
    }

    let legacy = Pbkdf2Hasher::new(MIN_PBKDF2_ITERATIONS).hash(b"s3cret-pass");
    assert!(legacy.starts_with("pbkdf2-sha256$i=210000$"), "{legacy}");
    assert!(production.verify(b"s3cret-pass", &legacy));

    let knob = Pbkdf2Hasher::new(1_000);
    assert!(knob.verify(b"s3cret-pass", &knob.hash(b"s3cret-pass")));
    let below = Pbkdf2Hasher::new(999).hash(b"s3cret-pass");
    assert!(
        !knob.verify(b"s3cret-pass", &below),
        "999 is below the floor of 1,000"
    );
}

#[test]
fn totp_matches_within_skew_and_rejects_outside() {
    let secret = TotpSecret::new(b"0123456789abcdef0123456789abcdef".to_vec());
    let t = 1_700_000_000u64;
    let code = secret.code_at(t);
    assert_eq!(code.len(), 6);
    assert!(secret.verify(&code, t));
    // ±1 step (30s) still verifies (skew tolerance).
    assert!(secret.verify(&code, t + 29));
    assert!(secret.verify(&code, t.saturating_sub(29)));
    // Two steps away does not.
    assert!(!secret.verify(&code, t + 61));
    assert!(!secret.verify("000000", t));
}

#[test]
fn webauthn_assertion_verifies_and_fails_closed() {
    let provider = fips_provider();
    let sig = provider.sig_for(PASSKEY_SIG_ALG).unwrap();
    let (vk, sk) = sig.keypair(&mut SeedRng(7)).unwrap();

    let cred = WebAuthnCredential {
        credential_id: b"cred-1".to_vec(),
        public_key: vk,
        rp_id: "midgard.local".into(),
    };
    let challenge = b"server-issued-challenge-bytes";

    // The authenticator builds client-data + authenticator-data and signs.
    let client_data = WebAuthnCredential::client_data_json(challenge, "https://midgard.local");
    let auth_data = cred.authenticator_data(0x01 /* user present */, 1);
    let payload = WebAuthnCredential::signed_payload(&auth_data, &client_data);
    let signature = sig.sign(&sk, &payload).unwrap();

    let good = AuthAssertion {
        credential_id: b"cred-1".to_vec(),
        authenticator_data: auth_data.clone(),
        client_data_json: client_data.clone(),
        signature: signature.clone(),
    };
    cred.verify_assertion(&provider, challenge, &good)
        .expect("genuine assertion verifies");

    // Wrong challenge → rejected.
    assert!(cred
        .verify_assertion(&provider, b"different-challenge", &good)
        .is_err());

    // User-present flag clear → rejected.
    let no_up = AuthAssertion {
        authenticator_data: cred.authenticator_data(0x00, 1),
        ..good.clone()
    };
    // Re-sign for the altered authenticator data so only the UP flag is at fault.
    let np_payload = WebAuthnCredential::signed_payload(&no_up.authenticator_data, &client_data);
    let no_up = AuthAssertion {
        signature: sig.sign(&sk, &np_payload).unwrap(),
        ..no_up
    };
    assert!(cred.verify_assertion(&provider, challenge, &no_up).is_err());

    // Tampered signature → rejected.
    let (_vk2, sk2) = sig.keypair(&mut SeedRng(8)).unwrap();
    let forged = AuthAssertion {
        signature: sig.sign(&sk2, &payload).unwrap(),
        ..good
    };
    assert!(cred
        .verify_assertion(&provider, challenge, &forged)
        .is_err());
}
