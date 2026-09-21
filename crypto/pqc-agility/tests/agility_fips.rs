//! AC-M-8: with the production FIPS backend, algorithms are negotiated, versioned,
//! and hot-swappable **without data migration** — an artefact created under one
//! algorithm still decodes after the registry changes, because its algorithm id
//! travels with it. [NFR-PQ-4]
#![cfg(feature = "backend-fips")]

use pqc_agility::backend::fips::{FipsKem, FipsSig};
use pqc_agility::{negotiate_kem, CryptoProvider, KemSuiteId, Rng, SigId};

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

/// Start with only ML-KEM-768 registered, seal data, then **hot-add** ML-KEM-1024
/// and swap negotiation preference — the old 768 ciphertext still decapsulates
/// (no migration), and new data uses 1024.
#[test]
fn hot_add_algorithm_leaves_old_ciphertext_valid() {
    let mut p = CryptoProvider::new();
    p.register_kem(Box::new(FipsKem::new(KemSuiteId::HybridX25519MlKem768)));

    // Seal under the only available suite.
    let kem = p.kem_for(KemSuiteId::HybridX25519MlKem768).unwrap();
    let (pk, sk) = kem.keypair(&mut SeedRng(1)).unwrap();
    let (old_ct, old_ss) = kem.encapsulate(&pk, &mut SeedRng(2)).unwrap();

    // Only 768 is offerable so far.
    assert_eq!(
        p.capabilities().kem_suites,
        vec![KemSuiteId::HybridX25519MlKem768]
    );

    // Hot-add ML-KEM-1024 at runtime (no restart, no data touched).
    p.register_kem(Box::new(FipsKem::new(KemSuiteId::HybridX25519MlKem1024)));

    // The OLD ciphertext — tagged with suite 768 — still decapsulates to the same
    // shared secret. No re-encryption / migration was needed.
    let recovered = p
        .kem_for(KemSuiteId::HybridX25519MlKem768)
        .unwrap()
        .decapsulate(&sk, &old_ct)
        .unwrap();
    assert_eq!(recovered.expose(), old_ss.expose());

    // New data can now use 1024.
    let kem1024 = p.kem_for(KemSuiteId::HybridX25519MlKem1024).unwrap();
    let (pk2, sk2) = kem1024.keypair(&mut SeedRng(3)).unwrap();
    let (ct2, ss2) = kem1024.encapsulate(&pk2, &mut SeedRng(4)).unwrap();
    let ss2b = kem1024.decapsulate(&sk2, &ct2).unwrap();
    assert_eq!(ss2.expose(), ss2b.expose());
}

/// Negotiation hot-swaps as algorithms are disabled and re-enabled — no data is
/// affected, only which suite new sessions pick.
#[test]
fn negotiation_hot_swaps_on_enable_disable() {
    let mut p = CryptoProvider::new();
    p.register_kem(Box::new(FipsKem::new(KemSuiteId::HybridX25519MlKem768)));
    p.register_kem(Box::new(FipsKem::new(KemSuiteId::HybridX25519MlKem1024)));

    // A peer that offers both; local preference has 768 first (default order).
    let peer = vec![
        KemSuiteId::HybridX25519MlKem768,
        KemSuiteId::HybridX25519MlKem1024,
    ];
    let chosen = negotiate_kem(&p.capabilities().kem_suites, &peer).unwrap();
    assert_eq!(chosen, KemSuiteId::HybridX25519MlKem768);

    // Disable 768 → negotiation swaps to 1024 with no other change.
    p.disable_kem(KemSuiteId::HybridX25519MlKem768);
    let chosen = negotiate_kem(&p.capabilities().kem_suites, &peer).unwrap();
    assert_eq!(chosen, KemSuiteId::HybridX25519MlKem1024);

    // Re-enable → back to 768.
    p.enable_kem(KemSuiteId::HybridX25519MlKem768);
    let chosen = negotiate_kem(&p.capabilities().kem_suites, &peer).unwrap();
    assert_eq!(chosen, KemSuiteId::HybridX25519MlKem768);
}

/// The same holds for signatures: a signature made under one algorithm still
/// verifies after another signature algorithm is added.
#[test]
fn hot_add_signature_algorithm_leaves_old_signature_valid() {
    let mut p = CryptoProvider::new();
    p.register_sig(Box::new(FipsSig::new(SigId::MlDsa65)));

    let scheme = p.sig_for(SigId::MlDsa65).unwrap();
    let (vk, sk) = scheme.keypair(&mut SeedRng(5)).unwrap();
    let msg = b"artefact provenance";
    let old_sig = scheme.sign(&sk, msg).unwrap();

    // Hot-add ML-DSA-87.
    p.register_sig(Box::new(FipsSig::new(SigId::MlDsa87)));

    // The old ML-DSA-65 signature still verifies (its alg id travels with it).
    p.sig_for(SigId::MlDsa65)
        .unwrap()
        .verify(&vk, msg, &old_sig)
        .expect("old signature still valid after adding a new algorithm");
}
