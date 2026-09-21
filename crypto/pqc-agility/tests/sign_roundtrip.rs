//! Signature round-trip + tamper/wrong-key/id-mismatch tests (test backend).
#![cfg(feature = "test-backend")]

use pqc_agility::backend::test_backend::provider;
use pqc_agility::{SeedRng, SigId, Signature, VerifyingKey};

#[test]
fn sign_verify_roundtrip() {
    let p = provider();
    let mut rng = SeedRng::new(1);
    for alg in p.capabilities().sig_algs {
        let scheme = p.sig_for(alg).unwrap();
        let (vk, sk) = scheme.keypair(&mut rng).unwrap();
        let sig = scheme.sign(&sk, b"hello midgard").unwrap();
        assert!(scheme.verify(&vk, b"hello midgard", &sig).is_ok());
    }
}

#[test]
fn tampered_message_fails() {
    let p = provider();
    let mut rng = SeedRng::new(2);
    let scheme = p.sig_for(SigId::MlDsa65).unwrap();
    let (vk, sk) = scheme.keypair(&mut rng).unwrap();
    let sig = scheme.sign(&sk, b"original").unwrap();
    assert!(scheme.verify(&vk, b"tampered", &sig).is_err());
}

#[test]
fn wrong_key_fails() {
    let p = provider();
    let mut rng = SeedRng::new(3);
    let scheme = p.sig_for(SigId::MlDsa65).unwrap();
    let (_vk, sk) = scheme.keypair(&mut rng).unwrap();
    let (vk2, _sk2) = scheme.keypair(&mut rng).unwrap();
    let sig = scheme.sign(&sk, b"msg").unwrap();
    assert!(scheme.verify(&vk2, b"msg", &sig).is_err());
}

#[test]
fn tampered_signature_fails() {
    let p = provider();
    let mut rng = SeedRng::new(4);
    let scheme = p.sig_for(SigId::MlDsa65).unwrap();
    let (vk, sk) = scheme.keypair(&mut rng).unwrap();
    let sig = scheme.sign(&sk, b"msg").unwrap();
    let mut bad = sig.as_bytes().to_vec();
    bad[0] ^= 0x01;
    let bad_sig = Signature::new(sig.alg(), bad);
    assert!(scheme.verify(&vk, b"msg", &bad_sig).is_err());
}

#[test]
fn algorithm_mismatch_is_rejected() {
    let p = provider();
    let mut rng = SeedRng::new(5);
    let scheme = p.sig_for(SigId::MlDsa65).unwrap();
    let (vk, sk) = scheme.keypair(&mut rng).unwrap();
    let sig = scheme.sign(&sk, b"msg").unwrap();
    // A verifying key claiming a different algorithm must be rejected.
    let wrong_vk = VerifyingKey::new(SigId::MlDsa87, vk.as_bytes().to_vec());
    assert!(scheme.verify(&wrong_vk, b"msg", &sig).is_err());
}
