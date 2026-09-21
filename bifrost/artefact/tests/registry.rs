//! Artefact registry: signed artefacts with SBOM attestation (AC-B-4) and
//! fail-closed rejection of unsigned/tampered artefacts (AC-B-1). [FR-B-1] [FR-B-4]
#![cfg(feature = "backend-fips")]

use bifrost_artefact::{
    build_and_sign, generate_sbom, verify, ArtefactRegistry, SbomComponent, VerifyError,
};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::hash::sha256;
use pqc_agility::{Rng, SignatureScheme};

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

fn release_keys() -> (pqc_agility::VerifyingKey, pqc_agility::SigningKey) {
    FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
        .keypair(&mut SeedRng(1))
        .unwrap()
}

fn sample_sbom() -> String {
    generate_sbom(
        "forge-pipeline",
        "1.4.0",
        "service",
        vec![
            SbomComponent::new("pqc-agility", "0.0.0"),
            SbomComponent::new("forge-engine", "0.0.0"),
        ],
    )
}

#[test]
fn ac_b_4_published_release_has_verifiable_sbom_and_ml_dsa_signature() {
    let (vk, sk) = release_keys();
    let sbom = sample_sbom();
    let artefact = build_and_sign(
        "forge-pipeline",
        "1.4.0",
        "service",
        b"DEPLOYABLE-BYTES-v1".to_vec(),
        sbom.clone(),
        &sk,
    );

    // A CycloneDX SBOM is attached and the ML-DSA signature verifies. [AC-B-4]
    assert!(artefact.sbom.contains("\"bomFormat\": \"CycloneDX\""));
    assert_eq!(artefact.manifest.sbom_sha256, sha256(sbom.as_bytes()));
    assert!(verify(&vk, &artefact).is_ok());
}

#[test]
fn ac_b_1_unsigned_or_tampered_artefact_is_rejected_fail_closed() {
    let (vk, sk) = release_keys();
    let artefact = build_and_sign(
        "forge-pipeline",
        "1.4.0",
        "service",
        b"DEPLOYABLE-BYTES-v1".to_vec(),
        sample_sbom(),
        &sk,
    );

    // Tampered payload → payload digest mismatch.
    let mut t1 = artefact.clone();
    t1.payload = b"MALICIOUS-BYTES".to_vec();
    assert_eq!(verify(&vk, &t1), Err(VerifyError::PayloadDigestMismatch));

    // Tampered SBOM → SBOM digest mismatch.
    let mut t2 = artefact.clone();
    t2.sbom.push_str("\n<tamper>");
    assert_eq!(verify(&vk, &t2), Err(VerifyError::SbomDigestMismatch));

    // Wrong signing key → bad signature (an "unsigned" artefact under our trust).
    let (_other_vk, other_sk) = FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
        .keypair(&mut SeedRng(999))
        .unwrap();
    let forged = build_and_sign(
        "forge-pipeline",
        "1.4.0",
        "service",
        b"DEPLOYABLE-BYTES-v1".to_vec(),
        sample_sbom(),
        &other_sk,
    );
    assert_eq!(verify(&vk, &forged), Err(VerifyError::BadSignature));
}

#[test]
fn registry_verifies_on_ingest_and_before_deploy() {
    let (vk, sk) = release_keys();
    let mut registry = ArtefactRegistry::new(vk);

    // A tampered artefact never enters the registry.
    let good = build_and_sign(
        "forge-pipeline",
        "1.4.0",
        "service",
        b"BYTES".to_vec(),
        sample_sbom(),
        &sk,
    );
    let mut bad = good.clone();
    bad.payload = b"tampered".to_vec();
    assert!(registry.ingest(bad).is_err());
    assert!(registry.is_empty());

    // The good one ingests and re-verifies at deploy time.
    let r = registry.ingest(good).unwrap();
    assert_eq!(r.id, "forge-pipeline");
    assert_eq!(registry.len(), 1);
    assert!(registry
        .verify_for_deploy("forge-pipeline", "1.4.0")
        .is_ok());

    // An unknown artefact fails closed at the deploy gate.
    assert!(registry.verify_for_deploy("unknown", "9.9.9").is_err());
}

#[test]
fn sbom_is_deterministic() {
    // Same inputs (even with components in a different order) → byte-identical SBOM.
    let a = generate_sbom(
        "x",
        "1.0",
        "service",
        vec![SbomComponent::new("b", "2"), SbomComponent::new("a", "1")],
    );
    let b = generate_sbom(
        "x",
        "1.0",
        "service",
        vec![SbomComponent::new("a", "1"), SbomComponent::new("b", "2")],
    );
    assert_eq!(a, b);
}
