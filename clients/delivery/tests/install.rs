//! AC-C-7 — a client artefact is verified before install/update and fails closed on
//! tamper — and AC-C-12 — air-gapped install from a signed offline bundle, offline.
#![cfg(feature = "backend-fips")]

use bifrost_artefact::{build_and_sign, generate_sbom, SbomComponent, SignedArtefact};
use bifrost_bundle::build_bundle;
use midgard_client_delivery::{
    install_from_bundle, verify_and_install, verify_and_update, DeliveryError,
};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::{Rng, SignatureScheme, SigningKey, VerifyingKey};

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

fn keys(seed: u64) -> (VerifyingKey, SigningKey) {
    FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
        .keypair(&mut SeedRng(seed))
        .unwrap()
}

fn client_artefact(id: &str, version: &str, sk: &SigningKey) -> SignedArtefact {
    let sbom = generate_sbom(id, version, "client", vec![SbomComponent::new("core", "1")]);
    build_and_sign(
        id,
        version,
        "client",
        format!("{id}-{version}-app-payload").into_bytes(),
        sbom,
        sk,
    )
}

#[test]
fn a_signed_client_artefact_installs() {
    let (vk, sk) = keys(1);
    let art = client_artefact("midgard-desktop", "1.0.0", &sk);
    let installed = verify_and_install(&vk, &art).unwrap();
    assert_eq!(installed.id, "midgard-desktop");
    assert_eq!(installed.version, "1.0.0");
    assert_eq!(installed.component, "client");
}

#[test]
fn a_tampered_artefact_fails_closed_before_install() {
    let (vk, sk) = keys(2);
    let mut art = client_artefact("midgard-desktop", "1.0.0", &sk);
    // Tamper with the payload after signing.
    art.payload.push(0xff);
    let err = verify_and_install(&vk, &art).unwrap_err();
    assert!(matches!(err, DeliveryError::Verify(_)));
}

#[test]
fn an_artefact_signed_by_an_untrusted_key_fails_closed() {
    let (_vk, sk) = keys(3);
    let (attacker_vk, _attacker_sk) = keys(999); // not the pinned release key
    let art = client_artefact("midgard-desktop", "1.0.0", &sk);
    let err = verify_and_install(&attacker_vk, &art).unwrap_err();
    assert!(matches!(err, DeliveryError::Verify(_)));
}

#[test]
fn a_non_client_artefact_is_refused() {
    let (vk, sk) = keys(4);
    // A model bundle, not a client app — the client must not install it.
    let sbom = generate_sbom(
        "mistral-int4",
        "5.2",
        "model-bundle",
        vec![SbomComponent::new("mistral-int4", "5.2")],
    );
    let art = build_and_sign(
        "mistral-int4",
        "5.2",
        "model-bundle",
        b"weights".to_vec(),
        sbom,
        &sk,
    );
    let err = verify_and_install(&vk, &art).unwrap_err();
    assert!(matches!(err, DeliveryError::WrongComponent { .. }));
}

#[test]
fn an_update_must_target_the_installed_artefact() {
    let (vk, sk) = keys(5);
    let v1 = client_artefact("midgard-desktop", "1.0.0", &sk);
    let installed = verify_and_install(&vk, &v1).unwrap();

    // A genuine update to the same artefact is accepted.
    let v2 = client_artefact("midgard-desktop", "1.1.0", &sk);
    let updated = verify_and_update(&vk, &installed, &v2).unwrap();
    assert_eq!(updated.version, "1.1.0");

    // An artefact for a different app is refused as an update.
    let other = client_artefact("some-other-app", "9.9.9", &sk);
    let err = verify_and_update(&vk, &installed, &other).unwrap_err();
    assert!(matches!(err, DeliveryError::WrongArtefact { .. }));
}

#[test]
fn air_gapped_install_from_a_signed_bundle_offline() {
    let (vk, sk) = keys(6);
    let bundle = build_bundle(
        "client-release-2026.07",
        "stable",
        vec![
            client_artefact("midgard-desktop", "1.0.0", &sk),
            client_artefact("midgard-mobile", "1.0.0", &sk),
        ],
        &sk,
    );

    // No network, no store — the bundle is verified and installed entirely offline.
    let installed = install_from_bundle(&vk, &bundle).unwrap();
    assert_eq!(installed.len(), 2);
    assert!(installed.iter().all(|a| a.component == "client"));
}

#[test]
fn a_tampered_bundle_fails_closed_offline() {
    let (vk, sk) = keys(7);
    let mut bundle = build_bundle(
        "client-release-2026.07",
        "stable",
        vec![client_artefact("midgard-desktop", "1.0.0", &sk)],
        &sk,
    );
    // Tamper with a contained artefact after the bundle was signed.
    bundle.artefacts[0].payload.push(0x00);
    let err = install_from_bundle(&vk, &bundle).unwrap_err();
    assert!(matches!(err, DeliveryError::Bundle(_)));
}
