//! Unit coverage for the credential primitives: password hashing (Argon2id, and
//! legacy PBKDF2 within its cost bounds), TOTP, and WebAuthn assertion verification,
//! with its origin, user-verification and signature counter checks. [RC-29]
//! [NFR-SEC-2] [MG-SEC-11] [VLD-ANB-MIDGARD-0025] [VLD-ANB-MIDGARD-0084]
#![cfg(feature = "backend-fips")]

use iam::enc::{b64url, unb64url};
use iam::mfa::{
    AuthAssertion, ClientData, WebAuthnCredential, WebAuthnError, WebAuthnPolicy, PASSKEY_SIG_ALG,
};
use std::cell::RefCell;
use std::time::{Duration, Instant};

use iam::{Argon2idHasher, PasswordHasher, Pbkdf2Hasher, TotpSecret, MIN_PBKDF2_ITERATIONS};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, SigningKey};

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

/// [AC-57.3] [VLD-ANB-MIDGARD-0026] `verify_step` returns the step a code matched and
/// refuses that step and every earlier one once it is the last accepted, while the
/// next step's code still verifies within the one-step skew.
#[test]
fn totp_verify_step_returns_the_step_and_refuses_used_or_older_steps() {
    let secret = TotpSecret::new(b"0123456789abcdef0123456789abcdef".to_vec());
    let t = 1_700_000_000u64;
    let step = t / 30;
    // With nothing accepted yet, each code within the skew returns its own step.
    assert_eq!(secret.verify_step(&secret.code_at(t), t, None), Some(step));
    assert_eq!(
        secret.verify_step(&secret.code_at(t - 30), t, None),
        Some(step - 1)
    );
    // Once the current step is the last accepted, it and the one before are refused.
    let last = Some(step);
    assert_eq!(secret.verify_step(&secret.code_at(t), t, last), None);
    assert_eq!(secret.verify_step(&secret.code_at(t - 30), t, last), None);
    // The next step's code is still accepted, and reported as the next step.
    assert_eq!(
        secret.verify_step(&secret.code_at(t + 30), t, last),
        Some(step + 1)
    );
    // Outside the skew, or a wrong code, nothing matches.
    assert_eq!(secret.verify_step(&secret.code_at(t + 61), t, None), None);
    assert_eq!(secret.verify_step("000000", t, None), None);
    // The stateless form is unchanged for existing callers.
    assert!(secret.verify(&secret.code_at(t), t));
}

/// The origin the WebAuthn tests' relying party serves.
const ORIGIN: &str = "https://console.enclosure.local";
/// The challenge the WebAuthn tests' server issued.
const CHALLENGE: &[u8] = b"server-issued-challenge-bytes";
/// The user-present and user-verified flags.
const UP: u8 = 0x01;
const UV: u8 = 0x04;

/// A passkey with its signing key, as a local authenticator holds it.
fn passkey(seed: u64) -> (WebAuthnCredential, SigningKey) {
    let provider = fips_provider();
    let sig = provider.sig_for(PASSKEY_SIG_ALG).unwrap();
    let (vk, sk) = sig.keypair(&mut SeedRng(seed)).unwrap();
    let cred = WebAuthnCredential {
        credential_id: b"cred-1".to_vec(),
        public_key: vk,
        rp_id: "midgard.local".into(),
        sign_count: 0,
    };
    (cred, sk)
}

/// The relying party's policy: this origin, user verification required.
fn policy() -> WebAuthnPolicy {
    WebAuthnPolicy {
        origins: vec![ORIGIN.into()],
        require_uv: true,
    }
}

/// An assertion `sk` signs over authenticator data with `flags` and `count`, and
/// the client-data JSON of `client_data`.
fn assertion(
    cred: &WebAuthnCredential,
    sk: &SigningKey,
    flags: u8,
    count: u32,
    client_data: ClientData,
) -> AuthAssertion {
    let provider = fips_provider();
    let sig = provider.sig_for(PASSKEY_SIG_ALG).unwrap();
    let client_data_json = format!(
        r#"{{"type":"{}","challenge":"{}","origin":"{}"}}"#,
        client_data.type_,
        b64url(&client_data.challenge),
        client_data.origin
    )
    .into_bytes();
    let authenticator_data = cred.authenticator_data(flags, count);
    let payload = WebAuthnCredential::signed_payload(&authenticator_data, &client_data_json);
    AuthAssertion {
        credential_id: cred.credential_id.clone(),
        authenticator_data,
        client_data_json,
        signature: sig.sign(sk, &payload).unwrap(),
    }
}

/// The edge's client-data parser, as far as these tests need one (`iam` has no JSON
/// parser): the three fields in the order [`assertion`] writes them, nothing else.
fn canonical(raw: &[u8]) -> Result<ClientData, String> {
    let raw = std::str::from_utf8(raw).map_err(|e| e.to_string())?;
    let rest = raw
        .strip_prefix(r#"{"type":""#)
        .and_then(|r| r.strip_suffix(r#""}"#))
        .ok_or("not client data")?;
    let (type_, rest) = rest
        .split_once(r#"","challenge":""#)
        .ok_or("no challenge")?;
    let (challenge, origin) = rest.split_once(r#"","origin":""#).ok_or("no origin")?;
    Ok(ClientData {
        type_: type_.into(),
        challenge: unb64url(challenge).ok_or("challenge is not base64url")?,
        origin: origin.into(),
    })
}

/// Client data of a `get` for `challenge` at `origin`, as the edge parses it.
fn get(challenge: &[u8], origin: &str) -> ClientData {
    ClientData {
        type_: "webauthn.get".into(),
        challenge: challenge.to_vec(),
        origin: origin.into(),
    }
}

/// [RC-29] [VLD-ANB-MIDGARD-0084] A genuine assertion verifies and returns its
/// counter; each altered part is refused with its own error.
#[test]
fn webauthn_assertion_verifies_and_fails_closed() {
    let provider = fips_provider();
    let (cred, sk) = passkey(7);

    let good = assertion(&cred, &sk, UP | UV, 1, get(CHALLENGE, ORIGIN));
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &good, &canonical),
        Ok(1),
        "a genuine assertion verifies and presents its counter"
    );

    // Wrong challenge → rejected.
    assert_eq!(
        cred.verify_assertion(
            &provider,
            &policy(),
            b"different-challenge",
            &good,
            &canonical
        ),
        Err(WebAuthnError::ChallengeMismatch)
    );

    // Client data of another ceremony type, signed as such → rejected.
    let create = assertion(
        &cred,
        &sk,
        UP | UV,
        1,
        ClientData {
            type_: "webauthn.create".into(),
            ..get(CHALLENGE, ORIGIN)
        },
    );
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &create, &canonical),
        Err(WebAuthnError::BadClientData)
    );

    // User-present flag clear, re-signed so only the UP flag is at fault → rejected.
    let no_up = assertion(&cred, &sk, UV, 1, get(CHALLENGE, ORIGIN));
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &no_up, &canonical),
        Err(WebAuthnError::UserNotPresent)
    );

    // Tampered signature → rejected.
    let (_, sk2) = passkey(8);
    let forged = AuthAssertion {
        signature: assertion(&cred, &sk2, UP | UV, 1, get(CHALLENGE, ORIGIN)).signature,
        ..good.clone()
    };
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &forged, &canonical),
        Err(WebAuthnError::BadSignature)
    );

    // The default policy names no origin, so nothing verifies until one is configured.
    assert_eq!(
        cred.verify_assertion(
            &provider,
            &WebAuthnPolicy::default(),
            CHALLENGE,
            &good,
            &canonical
        ),
        Err(WebAuthnError::OriginMismatch)
    );
}

/// [AC-58.4] [VLD-ANB-MIDGARD-0084] The client data checked is read from the bytes the
/// signature covers, by the caller's parser: it is handed exactly the signed
/// `clientDataJSON`; client data signed for another origin is refused, and honest
/// client data put in its place after signing fails the signature; bytes the parser
/// refuses are bad client data. There is no other client data to supply.
#[test]
fn webauthn_client_data_is_read_from_the_signed_bytes() {
    let provider = fips_provider();
    let (cred, sk) = passkey(7);
    let good = assertion(&cred, &sk, UP | UV, 1, get(CHALLENGE, ORIGIN));
    let seen = RefCell::new(Vec::new());
    let spy = |raw: &[u8]| {
        seen.borrow_mut().push(raw.to_vec());
        canonical(raw)
    };
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &good, &spy),
        Ok(1)
    );
    assert_eq!(
        *seen.borrow(),
        vec![good.client_data_json.clone()],
        "the parser reads the signed bytes, once"
    );

    let phished = assertion(
        &cred,
        &sk,
        UP | UV,
        1,
        get(CHALLENGE, "https://evil.example"),
    );
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &phished, &canonical),
        Err(WebAuthnError::OriginMismatch)
    );
    let swapped = AuthAssertion {
        client_data_json: good.client_data_json.clone(),
        ..phished
    };
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &swapped, &canonical),
        Err(WebAuthnError::BadSignature),
        "client data that is not what was signed does not verify"
    );

    let garbled = AuthAssertion {
        client_data_json: b"not client data".to_vec(),
        ..good
    };
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &garbled, &canonical),
        Err(WebAuthnError::BadClientData)
    );
}

/// [AC-58.1] [VLD-ANB-MIDGARD-0084] A correctly signed assertion from an origin the
/// policy does not name is refused.
#[test]
fn webauthn_origin_mismatch_is_refused() {
    let provider = fips_provider();
    let (cred, sk) = passkey(7);
    let phished = assertion(
        &cred,
        &sk,
        UP | UV,
        1,
        get(CHALLENGE, "https://evil.example"),
    );
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &phished, &canonical),
        Err(WebAuthnError::OriginMismatch)
    );
    // The origin must match exactly, not by prefix.
    let lookalike = assertion(
        &cred,
        &sk,
        UP | UV,
        1,
        get(CHALLENGE, "https://console.enclosure.local.evil.example"),
    );
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &lookalike, &canonical),
        Err(WebAuthnError::OriginMismatch)
    );
}

/// [AC-58.2] [VLD-ANB-MIDGARD-0084] An assertion with the user present but not
/// verified is refused when the policy requires verification, and accepted when it
/// does not.
#[test]
fn webauthn_uv_is_required_by_policy() {
    let provider = fips_provider();
    let (cred, sk) = passkey(7);
    let unverified = assertion(&cred, &sk, UP, 1, get(CHALLENGE, ORIGIN));
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &unverified, &canonical),
        Err(WebAuthnError::UserNotVerified)
    );
    let lenient = WebAuthnPolicy {
        require_uv: false,
        ..policy()
    };
    assert_eq!(
        cred.verify_assertion(&provider, &lenient, CHALLENGE, &unverified, &canonical),
        Ok(1)
    );
}

/// [AC-58.3] [VLD-ANB-MIDGARD-0084] Against a stored counter of 10, assertions with
/// counters 10 and 5 are refused and 11 is accepted, returning 11; when both the
/// stored and the presented counter are 0 the check is skipped.
#[test]
fn webauthn_sign_count_must_increase() {
    let provider = fips_provider();
    let (cred, sk) = passkey(7);
    let used = WebAuthnCredential {
        sign_count: 10,
        ..cred.clone()
    };
    for count in [10, 5] {
        let replayed = assertion(&used, &sk, UP | UV, count, get(CHALLENGE, ORIGIN));
        assert_eq!(
            used.verify_assertion(&provider, &policy(), CHALLENGE, &replayed, &canonical),
            Err(WebAuthnError::CounterRegression),
            "counter {count} is not past 10"
        );
    }
    let next = assertion(&used, &sk, UP | UV, 11, get(CHALLENGE, ORIGIN));
    assert_eq!(
        used.verify_assertion(&provider, &policy(), CHALLENGE, &next, &canonical),
        Ok(11)
    );

    // An authenticator that keeps no counter presents 0 every time.
    let counterless = assertion(&cred, &sk, UP | UV, 0, get(CHALLENGE, ORIGIN));
    assert_eq!(
        cred.verify_assertion(&provider, &policy(), CHALLENGE, &counterless, &canonical),
        Ok(0)
    );
    // But once a counter has been seen, 0 is a regression.
    assert_eq!(
        used.verify_assertion(&provider, &policy(), CHALLENGE, &counterless, &canonical),
        Err(WebAuthnError::CounterRegression)
    );
}
