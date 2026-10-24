//! AC-B-3 — Given an air-gapped site, when a signed bundle is imported, then its
//! signature is verified offline before deploy. Also exercises the AC-MS-3 (model
//! bundle) and AC-C-7 (client) transit links. [FR-B-5]
#![cfg(feature = "backend-fips")]

use bifrost_artefact::{build_and_sign, generate_sbom, SbomComponent, SignedArtefact};
use bifrost_bundle::{build_bundle, import, BundleError, OfflineBundle};
use pqc_agility::backend::fips::FipsSig;
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

fn keys(seed: u64) -> (pqc_agility::VerifyingKey, pqc_agility::SigningKey) {
    FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
        .keypair(&mut SeedRng(seed))
        .unwrap()
}

fn artefact(
    id: &str,
    version: &str,
    component: &str,
    sk: &pqc_agility::SigningKey,
) -> SignedArtefact {
    let sbom = generate_sbom(id, version, component, vec![SbomComponent::new("dep", "1")]);
    build_and_sign(
        id,
        version,
        component,
        format!("{id}-{version}-payload").into_bytes(),
        sbom,
        sk,
    )
}

fn sample_bundle(sk: &pqc_agility::SigningKey) -> OfflineBundle {
    build_bundle(
        "release-2026.07",
        "stable-2026.07",
        vec![
            artefact("forge-pipeline", "1.4.0", "service", sk),
            artefact("heimdall", "0.9.0", "service", sk),
        ],
        sk,
    )
}

#[test]
fn ac_b_3_air_gapped_import_verifies_offline_then_admits_artefacts() {
    let (vk, sk) = keys(1);
    let bundle = sample_bundle(&sk);

    // The air-gapped site verifies the bundle signature + contents fully offline.
    let refs = import(&vk, &bundle).expect("valid bundle imports");
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].id, "forge-pipeline");
    assert_eq!(refs[1].id, "heimdall");
}

#[test]
fn ac_b_3_bundle_rejected_offline_on_signature_failure() {
    let (vk, sk) = keys(2);
    let bundle = sample_bundle(&sk);

    // Wrong trust key → bundle signature fails → rejected before any deploy.
    let (other_vk, _other_sk) = keys(777);
    assert_eq!(
        import(&other_vk, &bundle),
        Err(BundleError::BadBundleSignature)
    );

    // Tampered contained artefact payload → the per-artefact verification catches
    // it (the payload no longer hashes to the digest the manifest signed over).
    let mut tampered = bundle.clone();
    tampered.artefacts[0].payload = b"swapped-payload".to_vec();
    assert!(matches!(
        import(&vk, &tampered),
        Err(BundleError::Artefact {
            index: 0,
            error: bifrost_artefact::VerifyError::PayloadDigestMismatch
        })
    ));

    // Adding an artefact not listed in the signed manifest → TOC mismatch.
    let mut extra = bundle.clone();
    extra
        .artefacts
        .push(artefact("stowaway", "0.0.1", "service", &sk));
    assert_eq!(import(&vk, &extra), Err(BundleError::ManifestMismatch));
}

#[test]
fn ac_ms_3_and_c_7_model_and_client_bundles_transit_the_same_workflow() {
    let (vk, sk) = keys(3);
    // A Mimir model bundle (AC-MS-3) and a client artefact (AC-C-7) travel as
    // component artefacts inside the same signed-bundle workflow.
    let bundle = build_bundle(
        "supply-2026.07",
        "models+clients",
        vec![
            artefact("mistral-small-3.2-q4", "3.2.0", "model-bundle", &sk),
            artefact("client-linux-x64", "3.1.0", "client", &sk),
        ],
        &sk,
    );

    let refs = import(&vk, &bundle).expect("model+client bundle imports offline");
    assert_eq!(refs.len(), 2);
    assert!(refs.iter().any(|r| r.id == "mistral-small-3.2-q4"));
    assert!(refs.iter().any(|r| r.id == "client-linux-x64"));

    // Rejected offline if the signature does not verify (Veldris/release key mismatch).
    let (wrong_vk, _sk) = keys(888);
    assert!(import(&wrong_vk, &bundle).is_err());
}
