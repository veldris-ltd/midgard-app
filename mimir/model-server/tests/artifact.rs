//! AC-MI-7: a model artefact of any supported family loads only if its Veldris
//! ML-DSA signature verifies, its licence policy is met, it is not revoked, and
//! the node satisfies its hardware profile — otherwise loading FAILS CLOSED.
//! (Phase 3 / AC-MS-2 generalises this into the Model Registry + air-gap supply.)
//! [AC-MI-7] [NFR-PQ-3]
#![cfg(feature = "backend-fips")]

use model_server::{
    sign_manifest, HwProfile, Licence, LicencePolicy, LoadError, ModelLoader, ModelManifest,
    NodeProfile,
};
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

/// An Veldris signing keypair (ML-DSA-65).
fn veldris_keys() -> (pqc_agility::VerifyingKey, pqc_agility::SigningKey) {
    FipsSig::new(model_server::VELDRIS_MODEL_SIG_ALG)
        .keypair(&mut SeedRng(42))
        .unwrap()
}

fn manifest(id: &str, family: &str, min_vram: u32, redistribution_ok: bool) -> ModelManifest {
    ModelManifest {
        id: id.into(),
        family: family.into(),
        version: "1.0".into(),
        sha256: [7u8; 32],
        hw: HwProfile::single_gpu(min_vram),
        licence: Licence {
            name: "test-licence".into(),
            osi_approved: redistribution_ok,
            redistribution_ok,
        },
        weights: None,
    }
}

fn gpu_node(vram: u32) -> NodeProfile {
    NodeProfile {
        gpus: 1,
        vram_gb_per_gpu: vram,
        accel: true,
        multinode: false,
        cpu_cores: 16,
        ram_gb: 64,
        in_container: true,
    }
}

#[test]
fn ac_mi_7_valid_artefact_of_each_family_loads() {
    let (vk, sk) = veldris_keys();
    let loader = ModelLoader::new(vk, LicencePolicy::default());
    let node = gpu_node(80);

    for family in ["GLM", "Nemotron", "Gemma"] {
        let art = sign_manifest(manifest(&format!("{family}-1"), family, 24, true), &sk);
        let loaded = loader
            .verify_and_load(&art, &node)
            .unwrap_or_else(|e| panic!("{family} should load, got {e:?}"));
        assert_eq!(loaded.family, family);
    }
}

#[test]
fn ac_mi_7_tampered_manifest_fails_closed() {
    let (vk, sk) = veldris_keys();
    let loader = ModelLoader::new(vk, LicencePolicy::default());
    let mut art = sign_manifest(manifest("glm-1", "GLM", 24, true), &sk);
    // Tamper after signing — the signature no longer covers the manifest.
    art.manifest.version = "9.9-evil".into();
    assert_eq!(
        loader.verify_and_load(&art, &gpu_node(80)),
        Err(LoadError::BadSignature)
    );
}

#[test]
fn ac_mi_7_wrong_signing_key_fails_closed() {
    let (_vk, sk) = veldris_keys();
    // The loader trusts a DIFFERENT key than the one that signed the artefact.
    let (rogue_vk, _rogue_sk) = FipsSig::new(model_server::VELDRIS_MODEL_SIG_ALG)
        .keypair(&mut SeedRng(999))
        .unwrap();
    let loader = ModelLoader::new(rogue_vk, LicencePolicy::default());
    let art = sign_manifest(manifest("glm-1", "GLM", 24, true), &sk);
    assert_eq!(
        loader.verify_and_load(&art, &gpu_node(80)),
        Err(LoadError::BadSignature)
    );
}

#[test]
fn ac_mi_7_hardware_ineligible_fails_closed() {
    let (vk, sk) = veldris_keys();
    let loader = ModelLoader::new(vk, LicencePolicy::default());
    // Artefact needs 48GB; the node has 24GB.
    let art = sign_manifest(manifest("glm-big", "GLM", 48, true), &sk);
    assert_eq!(
        loader.verify_and_load(&art, &gpu_node(24)),
        Err(LoadError::HardwareIneligible)
    );
}

#[test]
fn ac_mi_7_licence_policy_fails_closed() {
    let (vk, sk) = veldris_keys();
    // Deployment requires redistributable models.
    let loader = ModelLoader::new(
        vk,
        LicencePolicy {
            require_redistribution_ok: true,
        },
    );
    let art = sign_manifest(manifest("gemma-1", "Gemma", 24, false), &sk);
    assert_eq!(
        loader.verify_and_load(&art, &gpu_node(80)),
        Err(LoadError::LicenceDenied)
    );
}

#[test]
fn ac_mi_7_revoked_artefact_fails_closed_even_with_valid_signature() {
    let (vk, sk) = veldris_keys();
    let mut loader = ModelLoader::new(vk, LicencePolicy::default());
    let art = sign_manifest(manifest("glm-cve", "GLM", 24, true), &sk);
    loader.revoke("glm-cve");
    assert_eq!(
        loader.verify_and_load(&art, &gpu_node(80)),
        Err(LoadError::Revoked)
    );
}
