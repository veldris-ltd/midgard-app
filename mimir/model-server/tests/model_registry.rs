//! Model Registry acceptance (SAD_02b §4, Prompt 3.b). The registry is the
//! in-enclosure system of record; on EVERY load it verifies the Veldris ML-DSA
//! signature, licence policy, and hardware eligibility, enforces lifecycle state,
//! and fails closed on any mismatch. [FR-MS-3] [FR-MS-4] [FR-MS-5] [AC-MS-2]
//!
//!   AC-MS-2  any-family artefact loads only if its ML-DSA signature verifies;
//!            fail closed on mismatch (supersedes AC-MI-7).
//!   AC-MS-5  a variant the node cannot serve is not loaded; the largest eligible
//!            variant is offered instead.
//!   AC-MS-6  a licence-policy-unmet model is refused exactly as a revoked one.
//!   AC-MS-7  a revoked artefact fails closed regardless of signature validity.
#![cfg(feature = "backend-fips")]

use model_server::{
    sign_manifest, HwProfile, Licence, LicencePolicy, LifecycleState, LoadError, ModelManifest,
    ModelRegistry, NodeProfile,
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

fn veldris_keys() -> (pqc_agility::VerifyingKey, pqc_agility::SigningKey) {
    FipsSig::new(model_server::VELDRIS_MODEL_SIG_ALG)
        .keypair(&mut SeedRng(42))
        .unwrap()
}

fn manifest(id: &str, family: &str, hw: HwProfile, redistribution_ok: bool) -> ModelManifest {
    ModelManifest {
        id: id.into(),
        family: family.into(),
        version: "1.0".into(),
        sha256: [7u8; 32],
        hw,
        licence: Licence {
            name: "test-licence".into(),
            osi_approved: redistribution_ok,
            redistribution_ok,
        },
        weights: None,
    }
}

fn gpu_node(gpus: u32, vram: u32) -> NodeProfile {
    NodeProfile {
        gpus,
        vram_gb_per_gpu: vram,
        accel: true,
        multinode: false,
        cpu_cores: 16,
        ram_gb: 64,
        in_container: true,
    }
}

// --- AC-MS-2: verify-on-load across families; fail closed on bad signature ----

#[test]
fn ac_ms_2_each_family_loads_from_registry_and_bad_signature_fails_closed() {
    let (vk, sk) = veldris_keys();
    let mut reg = ModelRegistry::new(vk, LicencePolicy::default());
    let node = gpu_node(1, 80);

    for family in ["Mistral", "Nemotron", "Gemma"] {
        let art = sign_manifest(
            manifest(
                &format!("{family}-1"),
                family,
                HwProfile::single_gpu(24),
                true,
            ),
            &sk,
        );
        reg.register(art, Some("sbom-ref".into()), Some("prov-ref".into()));
        let loaded = reg
            .load(&format!("{family}-1"), &node)
            .unwrap_or_else(|e| panic!("{family} should load, got {e:?}"));
        assert_eq!(loaded.family, family);
    }

    // A tampered-after-signing manifest: the signature no longer covers it.
    let mut art = sign_manifest(
        manifest("mistral-evil", "Mistral", HwProfile::single_gpu(24), true),
        &sk,
    );
    art.manifest.version = "9.9-evil".into();
    reg.register(art, None, None);
    assert_eq!(reg.load("mistral-evil", &node), Err(LoadError::BadSignature));

    // An artefact the registry does not know cannot be loaded.
    assert_eq!(reg.load("ghost", &node), Err(LoadError::NotRegistered));

    // Every attempt was audited (system of record). 5 loads above.
    assert_eq!(reg.load_events().len(), 5);
    // The bad-signature attempt recorded sig_verified = false; the good ones true.
    let evil = reg
        .load_events()
        .iter()
        .find(|e| e.artefact_id == "mistral-evil")
        .unwrap();
    assert!(!evil.sig_verified);
    let good = reg
        .load_events()
        .iter()
        .find(|e| e.artefact_id == "Mistral-1")
        .unwrap();
    assert!(good.sig_verified);
}

#[test]
fn ac_ms_2_staged_but_not_approved_is_not_loadable() {
    let (vk, sk) = veldris_keys();
    let mut reg = ModelRegistry::new(vk, LicencePolicy::default());
    let art = sign_manifest(
        manifest("mistral-staged", "Mistral", HwProfile::single_gpu(24), true),
        &sk,
    );
    reg.register_with_state(art, LifecycleState::Staged, None, None);
    assert_eq!(
        reg.load("mistral-staged", &gpu_node(1, 80)),
        Err(LoadError::NotApproved)
    );
}

// --- AC-MS-5: hardware eligibility + largest-eligible variant -----------------

#[test]
fn ac_ms_5_offers_largest_eligible_variant_and_refuses_the_too_big_one() {
    let (vk, sk) = veldris_keys();
    let mut reg = ModelRegistry::new(vk, LicencePolicy::default());

    // Three Mistral variants of increasing hardware demand.
    let small = sign_manifest(
        manifest("mistral-small", "Mistral", HwProfile::single_gpu(24), true),
        &sk,
    );
    let mid = sign_manifest(
        manifest("mistral-mid", "Mistral", HwProfile::single_gpu(48), true),
        &sk,
    );
    let large = sign_manifest(
        manifest("mistral-large", "Mistral", HwProfile::multi(2, 80, false), true),
        &sk,
    );
    reg.register(small, None, None);
    reg.register(mid, None, None);
    reg.register(large, None, None);

    // A single 80GB GPU node: the 2-GPU "large" variant is ineligible.
    let node = gpu_node(1, 80);

    // Directly loading the too-big variant fails closed.
    assert_eq!(
        reg.load("mistral-large", &node),
        Err(LoadError::HardwareIneligible)
    );

    // Resolution offers the largest variant the node CAN serve: the 48GB "mid".
    let offered = reg
        .resolve_largest_eligible("Mistral", &node)
        .expect("some Mistral variant is eligible");
    assert_eq!(offered.id(), "mistral-mid");

    // And loading the largest eligible variant succeeds.
    let loaded = reg.load_largest_eligible("Mistral", &node).unwrap();
    assert_eq!(loaded.id, "mistral-mid");

    // On a bigger node (2×80GB) the large variant becomes the largest eligible.
    let big_node = gpu_node(2, 80);
    assert_eq!(
        reg.resolve_largest_eligible("Mistral", &big_node).unwrap().id(),
        "mistral-large"
    );
}

#[test]
fn ac_ms_5_no_eligible_variant_fails_closed() {
    let (vk, sk) = veldris_keys();
    let mut reg = ModelRegistry::new(vk, LicencePolicy::default());
    // Only a variant needing a GPU exists; the node is CPU-only.
    let art = sign_manifest(
        manifest("mistral-gpu", "Mistral", HwProfile::single_gpu(24), true),
        &sk,
    );
    reg.register(art, None, None);

    let cpu = NodeProfile::cpu_only(8);
    assert!(reg.resolve_largest_eligible("Mistral", &cpu).is_none());
    assert_eq!(
        reg.load_largest_eligible("Mistral", &cpu),
        Err(LoadError::HardwareIneligible)
    );
}

// --- AC-MS-6: licence-policy-unmet refused exactly as a revoked artefact ------

#[test]
fn ac_ms_6_licence_unmet_is_refused_exactly_as_revoked() {
    let (vk, sk) = veldris_keys();
    // Deployment requires redistributable models.
    let mut reg = ModelRegistry::new(
        vk,
        LicencePolicy {
            require_redistribution_ok: true,
        },
    );
    let node = gpu_node(1, 80);

    // A non-redistributable (licence-unmet) artefact and a revoked artefact.
    let denied = sign_manifest(
        manifest("gemma-nonredist", "Gemma", HwProfile::single_gpu(24), false),
        &sk,
    );
    let revoked = sign_manifest(
        manifest("gemma-revoked", "Gemma", HwProfile::single_gpu(24), true),
        &sk,
    );
    reg.register(denied, None, None);
    reg.register(revoked, None, None);
    reg.revoke("gemma-revoked");

    // Both fail closed on direct load …
    assert_eq!(
        reg.load("gemma-nonredist", &node),
        Err(LoadError::LicenceDenied)
    );
    assert_eq!(reg.load("gemma-revoked", &node), Err(LoadError::Revoked));

    // … and neither is ever offered by resolution — the licence-unmet artefact is
    // excluded exactly as the revoked one is.
    assert!(reg.resolve_largest_eligible("Gemma", &node).is_none());
}

// --- AC-MS-7: revoked fails closed regardless of signature validity -----------

#[test]
fn ac_ms_7_revoked_fails_closed_even_with_valid_signature() {
    let (vk, sk) = veldris_keys();
    let mut reg = ModelRegistry::new(vk, LicencePolicy::default());
    let art = sign_manifest(
        manifest("mistral-cve", "Mistral", HwProfile::single_gpu(24), true),
        &sk,
    );
    reg.register(art, None, None);

    // Valid signature, but revoked → Revoked, not a successful load.
    reg.revoke("mistral-cve");
    assert_eq!(
        reg.load("mistral-cve", &gpu_node(1, 80)),
        Err(LoadError::Revoked)
    );
}

#[test]
fn ac_ms_7_revoked_beats_invalid_signature() {
    let (vk, sk) = veldris_keys();
    let mut reg = ModelRegistry::new(vk, LicencePolicy::default());

    // A tampered artefact (its signature would NOT verify) that is also revoked.
    let mut art = sign_manifest(
        manifest("mistral-bad", "Mistral", HwProfile::single_gpu(24), true),
        &sk,
    );
    art.manifest.version = "tampered".into();
    reg.register(art, None, None);
    reg.revoke("mistral-bad");

    // Revocation short-circuits: the load fails closed as Revoked, not
    // BadSignature — proving revocation holds "regardless of signature validity".
    let node = gpu_node(1, 80);
    assert_eq!(reg.load("mistral-bad", &node), Err(LoadError::Revoked));
    let ev = reg.load_events().last().unwrap();
    assert!(!ev.sig_verified); // signature was never reached
}
