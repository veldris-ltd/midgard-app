//! KEM round-trip + length + tamper tests over the deterministic test backend.
#![cfg(feature = "test-backend")]

use pqc_agility::backend::test_backend::provider;
use pqc_agility::{Ciphertext, KemSuiteId, SeedRng};

#[test]
fn roundtrip_every_suite() {
    let p = provider();
    let mut rng = SeedRng::new(0xC0FFEE);
    for suite in p.capabilities().kem_suites {
        let kem = p.kem_for(suite).unwrap();
        let (pk, sk) = kem.keypair(&mut rng).unwrap();
        let sizes = kem.sizes();
        assert_eq!(pk.as_bytes().len(), sizes.public_key);

        let (ct, ss1) = kem.encapsulate(&pk, &mut rng).unwrap();
        assert_eq!(ct.as_bytes().len(), sizes.ciphertext);

        let ss2 = kem.decapsulate(&sk, &ct).unwrap();
        assert_eq!(
            ss1.expose(),
            ss2.expose(),
            "suite {suite:?} secret mismatch"
        );
    }
}

#[test]
fn tampered_ciphertext_changes_secret() {
    let p = provider();
    let mut rng = SeedRng::new(7);
    let suite = KemSuiteId::MlKem768;
    let kem = p.kem_for(suite).unwrap();
    let (pk, sk) = kem.keypair(&mut rng).unwrap();
    let (ct, ss1) = kem.encapsulate(&pk, &mut rng).unwrap();

    let mut bad = ct.as_bytes().to_vec();
    bad[0] ^= 0x01;
    let bad_ct = Ciphertext::new(suite, bad);
    let ss_bad = kem.decapsulate(&sk, &bad_ct).unwrap();
    assert_ne!(ss1.expose(), ss_bad.expose());
}

#[test]
fn wrong_suite_key_is_rejected() {
    let p = provider();
    let mut rng = SeedRng::new(11);
    let kem768 = p.kem_for(KemSuiteId::MlKem768).unwrap();
    let kem1024 = p.kem_for(KemSuiteId::MlKem1024).unwrap();
    let (pk1024, _sk) = kem1024.keypair(&mut rng).unwrap();
    // Encapsulating an ML-KEM-1024 key with the ML-KEM-768 backend must error.
    assert!(kem768.encapsulate(&pk1024, &mut rng).is_err());
}
