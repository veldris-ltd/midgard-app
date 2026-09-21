//! Crypto-agility / hot-swap without data migration (AC-M-8).
#![cfg(feature = "test-backend")]

use pqc_agility::backend::test_backend::provider;
use pqc_agility::{negotiate_kem, KemSuiteId, SeedRng};

#[test]
fn disable_reenable_changes_negotiation() {
    let mut p = provider();
    // Disable the hybrids so pure suites are what remain offerable.
    p.disable_kem(KemSuiteId::HybridX25519MlKem768);
    p.disable_kem(KemSuiteId::HybridX25519MlKem1024);

    let peer = vec![KemSuiteId::MlKem768, KemSuiteId::MlKem1024];
    let sel = negotiate_kem(&p.capabilities().kem_suites, &peer).unwrap();
    assert_eq!(sel, KemSuiteId::MlKem1024, "prefers higher security");

    // Hot-swap: disable 1024 → negotiation drops to 768, no restart, no migration.
    p.disable_kem(KemSuiteId::MlKem1024);
    let sel2 = negotiate_kem(&p.capabilities().kem_suites, &peer).unwrap();
    assert_eq!(sel2, KemSuiteId::MlKem768);

    // A disabled algorithm is not retrievable (fail closed).
    assert!(p.kem_for(KemSuiteId::MlKem1024).is_err());

    // Re-enable → available again.
    p.enable_kem(KemSuiteId::MlKem1024);
    assert!(p.kem_for(KemSuiteId::MlKem1024).is_ok());
}

#[test]
fn old_ciphertext_still_decapsulates_after_enabling_another_algorithm() {
    let mut p = provider();
    let mut rng = SeedRng::new(42);

    // Make a ciphertext under ML-KEM-768.
    let (pk, sk, ct, ss) = {
        let kem = p.kem_for(KemSuiteId::MlKem768).unwrap();
        let (pk, sk) = kem.keypair(&mut rng).unwrap();
        let (ct, ss) = kem.encapsulate(&pk, &mut rng).unwrap();
        (pk, sk, ct, ss)
    };
    let _ = pk;

    // Enabling/disabling other algorithms must not affect existing artefacts —
    // the id travels with the ciphertext, so no data migration is needed.
    p.disable_kem(KemSuiteId::MlKem1024);
    p.enable_kem(KemSuiteId::MlKem1024);

    let kem = p.kem_for(KemSuiteId::MlKem768).unwrap();
    let ss2 = kem.decapsulate(&sk, &ct).unwrap();
    assert_eq!(ss.expose(), ss2.expose());
}
