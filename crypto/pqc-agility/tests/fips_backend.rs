//! Integration tests for the production **FIPS 203/204/205 + X25519** backend.
//!
//! Scope: this proves the *integration* is correct — KEM/signature round-trips
//! across every offered suite, hybrid secrets that bind both halves, tamper and
//! wrong-key rejection, cross-suite fail-closed, and that an injected RNG is
//! plumbed faithfully (same seed → same key). The exhaustive NIST ACVP
//! known-answer vectors live in the upstream fips20x crates' own test suites;
//! we do not re-litigate the lattice maths here, we verify our use of it.
#![cfg(feature = "backend-fips")]

use pqc_agility::backend::fips::provider;
use pqc_agility::{Ciphertext, KemPublicKey, KemSuiteId, Rng, SigId};

/// Deterministic splitmix64 RNG — local to the test so we can prove the injected
/// randomness reaches key generation (a fixed seed must give a fixed key).
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

const ALL_SUITES: [KemSuiteId; 4] = [
    KemSuiteId::MlKem768,
    KemSuiteId::MlKem1024,
    KemSuiteId::HybridX25519MlKem768,
    KemSuiteId::HybridX25519MlKem1024,
];

const ALL_SIGS: [SigId; 4] = [
    SigId::MlDsa44,
    SigId::MlDsa65,
    SigId::MlDsa87,
    SigId::SlhDsaSha2_128s,
];

#[test]
fn kem_roundtrip_every_suite() {
    let p = provider();
    for suite in ALL_SUITES {
        let kem = p.kem_for(suite).expect("suite registered");
        let mut rng = SeedRng(0xABCD_1234 ^ suite.as_u8() as u64);
        let (pk, sk) = kem.keypair(&mut rng).expect("keypair");
        assert_eq!(
            pk.as_bytes().len(),
            kem.sizes().public_key,
            "{suite:?} pk len"
        );
        assert_eq!(
            sk.expose().len(),
            kem.sizes().secret_key,
            "{suite:?} sk len"
        );

        let (ct, ss_enc) = kem.encapsulate(&pk, &mut rng).expect("encaps");
        assert_eq!(
            ct.as_bytes().len(),
            kem.sizes().ciphertext,
            "{suite:?} ct len"
        );
        let ss_dec = kem.decapsulate(&sk, &ct).expect("decaps");
        assert_eq!(
            ss_enc.expose(),
            ss_dec.expose(),
            "{suite:?}: encapsulated and decapsulated secret must match"
        );
    }
}

#[test]
fn kem_wrong_secret_key_yields_different_secret() {
    // ML-KEM is IND-CCA with implicit rejection: decapsulating with the wrong
    // key returns a (pseudo-random) *different* secret, not the real one.
    let p = provider();
    for suite in ALL_SUITES {
        let kem = p.kem_for(suite).unwrap();
        let (pk, _sk) = kem.keypair(&mut SeedRng(1)).unwrap();
        let (_pk2, sk2) = kem.keypair(&mut SeedRng(2)).unwrap();
        let (ct, ss_enc) = kem.encapsulate(&pk, &mut SeedRng(3)).unwrap();
        let ss_wrong = kem.decapsulate(&sk2, &ct).unwrap();
        assert_ne!(
            ss_enc.expose(),
            ss_wrong.expose(),
            "{suite:?}: wrong key must not recover the secret"
        );
    }
}

#[test]
fn kem_tampered_ciphertext_changes_secret() {
    let p = provider();
    for suite in ALL_SUITES {
        let kem = p.kem_for(suite).unwrap();
        let (pk, sk) = kem.keypair(&mut SeedRng(10)).unwrap();
        let (ct, ss_enc) = kem.encapsulate(&pk, &mut SeedRng(11)).unwrap();
        let mut bytes = ct.as_bytes().to_vec();
        bytes[0] ^= 0xFF; // flip a byte in the ML-KEM portion
        let tampered = Ciphertext::new(suite, bytes);
        let ss_dec = kem.decapsulate(&sk, &tampered).unwrap();
        assert_ne!(
            ss_enc.expose(),
            ss_dec.expose(),
            "{suite:?}: tampered ciphertext must not decapsulate to the same secret"
        );
    }
}

#[test]
fn kem_cross_suite_public_key_rejected() {
    let p = provider();
    // A public key from suite B handed to the KEM for suite A must fail closed.
    let kem_a = p.kem_for(KemSuiteId::MlKem768).unwrap();
    let kem_b = p.kem_for(KemSuiteId::MlKem1024).unwrap();
    let (pk_b, _sk_b) = kem_b.keypair(&mut SeedRng(20)).unwrap();
    let err = kem_a.encapsulate(&pk_b, &mut SeedRng(21)).unwrap_err();
    assert!(
        matches!(err, pqc_agility::CryptoError::AlgorithmMismatch { .. }),
        "expected AlgorithmMismatch, got {err:?}"
    );
}

#[test]
fn kem_wrong_length_public_key_rejected() {
    let p = provider();
    let kem = p.kem_for(KemSuiteId::MlKem768).unwrap();
    let bad = KemPublicKey::new(KemSuiteId::MlKem768, vec![0u8; 3]);
    let err = kem.encapsulate(&bad, &mut SeedRng(30)).unwrap_err();
    assert!(
        matches!(err, pqc_agility::CryptoError::LengthMismatch { .. }),
        "expected LengthMismatch, got {err:?}"
    );
}

#[test]
fn injected_rng_is_faithful_same_seed_same_key() {
    // Proves the RNG bridge is deterministic and actually feeds keygen: identical
    // seeds must produce byte-identical keys for every suite. This is also a
    // stability anchor — a change in the crypto path or the RNG plumbing trips it.
    let p = provider();
    for suite in ALL_SUITES {
        let kem = p.kem_for(suite).unwrap();
        let (pk1, sk1) = kem.keypair(&mut SeedRng(0xFEED)).unwrap();
        let (pk2, sk2) = kem.keypair(&mut SeedRng(0xFEED)).unwrap();
        assert_eq!(pk1.as_bytes(), pk2.as_bytes(), "{suite:?} pk determinism");
        assert_eq!(sk1.expose(), sk2.expose(), "{suite:?} sk determinism");
        // A different seed must give a different key.
        let (pk3, _) = kem.keypair(&mut SeedRng(0xBEEF)).unwrap();
        assert_ne!(pk1.as_bytes(), pk3.as_bytes(), "{suite:?} seed sensitivity");
    }
}

#[test]
fn sign_verify_roundtrip_every_alg() {
    let p = provider();
    let msg = b"midgard core: post-quantum service authentication";
    for alg in ALL_SIGS {
        let scheme = p.sig_for(alg).expect("alg registered");
        let (vk, sk) = scheme.keypair(&mut SeedRng(alg.as_u8() as u64)).unwrap();
        let sig = scheme.sign(&sk, msg).expect("sign");
        scheme
            .verify(&vk, msg, &sig)
            .expect("valid signature must verify");
    }
}

#[test]
fn tampered_message_and_signature_fail() {
    let p = provider();
    let msg = b"transfer authorised";
    for alg in ALL_SIGS {
        let scheme = p.sig_for(alg).unwrap();
        let (vk, sk) = scheme
            .keypair(&mut SeedRng(77 + alg.as_u8() as u64))
            .unwrap();
        let sig = scheme.sign(&sk, msg).unwrap();

        // Wrong message.
        assert!(
            scheme.verify(&vk, b"transfer denied", &sig).is_err(),
            "{alg:?}: altered message must not verify"
        );
        // Tampered signature.
        let mut sig_bytes = sig.as_bytes().to_vec();
        sig_bytes[0] ^= 0x01;
        let bad_sig = pqc_agility::Signature::new(alg, sig_bytes);
        assert!(
            scheme.verify(&vk, msg, &bad_sig).is_err(),
            "{alg:?}: tampered signature must not verify"
        );
    }
}

#[test]
fn wrong_verifying_key_fails() {
    let p = provider();
    let msg = b"root of trust";
    for alg in ALL_SIGS {
        let scheme = p.sig_for(alg).unwrap();
        let (_vk, sk) = scheme
            .keypair(&mut SeedRng(100 + alg.as_u8() as u64))
            .unwrap();
        let (vk_other, _) = scheme
            .keypair(&mut SeedRng(200 + alg.as_u8() as u64))
            .unwrap();
        let sig = scheme.sign(&sk, msg).unwrap();
        assert!(
            scheme.verify(&vk_other, msg, &sig).is_err(),
            "{alg:?}: signature must not verify under a different key"
        );
    }
}
