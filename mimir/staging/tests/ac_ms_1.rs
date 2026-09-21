//! AC-MS-1 — Given an upstream model, when staged, then native provenance is
//! verified where available, a CycloneDX SBOM is generated, the licence is
//! captured, and the bundle is re-signed under Veldris's ML-DSA key.
//! [FR-MS-1] [FR-MS-2] [NFR-PQ-3]
#![cfg(feature = "backend-fips")]

use model_server::{HwProfile, Licence, LicencePolicy, ModelLoader, NodeProfile};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::hash::sha256;
use pqc_agility::{Rng, SignatureScheme};
use staging::{
    finalize, prepare, DeclaredProvenance, FetchedModel, HumanLicenceGate, IntakeError,
    ReferenceFetcher, Tri, UpstreamSource, Verifiability,
};
use std::collections::BTreeMap;

// A deterministic RNG for the Veldris keypair (the staging signing key), matching
// the model-server artefact tests.
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

fn hex(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(LUT[(b >> 4) as usize] as char);
        s.push(LUT[(b & 0x0f) as usize] as char);
    }
    s
}

const GEMMA_CARD: &str = "\
# Gemma-2-9B

Licensed under the Google Gemma Terms of Use.

## Prohibited Uses:
- You may not use the model to generate disinformation.

## Provenance
Fine-tuned from google/gemma-2-9b.

## Safety
Evaluated for toxicity and bias.
";

fn gemma_source() -> UpstreamSource {
    UpstreamSource::HuggingFace {
        repo: "google/gemma-2-9b".into(),
        revision: "abc123".into(),
    }
}

/// Build a Gemma fixture. If `declared_digest` is set, it is the published
/// SHA-256 that provenance verification checks the weights against.
fn gemma_fixture(weights: &[u8], declared_digest: Option<String>, signed: bool) -> FetchedModel {
    let mut components = BTreeMap::new();
    components.insert("tokenizer.json".to_string(), b"{tokenizer}".to_vec());
    components.insert("config.json".to_string(), b"{config}".to_vec());
    FetchedModel {
        source: gemma_source(),
        proposed_id: "gemma-2-9b-1".into(),
        family: "Gemma".into(),
        version: "1.0".into(),
        hw: HwProfile::single_gpu(24),
        weights: weights.to_vec(),
        components,
        model_card: GEMMA_CARD.into(),
        metadata: format!(
            "source: huggingface:google/gemma-2-9b@abc123; vendor signature {}; commit abc123",
            if signed { "present (gpg)" } else { "absent" }
        ),
        declared: DeclaredProvenance {
            published_sha256_hex: declared_digest,
            vendor_signature: signed.then(|| "gpg:0xDEADBEEF".to_string()),
            hf_commit: Some("abc123".into()),
            pinned_revision_files: None,
        },
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
fn ac_ms_1_full_intake_verifies_provenance_sbom_licence_and_resigns() {
    let weights = b"GEMMA-WEIGHTS-BLOB-v1".to_vec();
    let digest = hex(&sha256(&weights));
    let fetcher = ReferenceFetcher::new().with(gemma_fixture(&weights, Some(digest.clone()), true));

    // --- Phase 1: prepare (fetch → provenance → SBOM → advisory assessment) ---
    let prep = prepare(&fetcher, &gemma_source()).expect("prepare should succeed");

    // Native provenance verified where available, verifiability recorded. [FR-MS-2]
    assert!(!prep.provenance.has_mismatch());
    assert!(prep.provenance.any_verified());
    assert_eq!(prep.provenance.weights_sha256_hex(), digest);
    let sha_check = prep
        .provenance
        .checks
        .iter()
        .find(|c| c.check == "sha256-weights")
        .expect("sha256 check present");
    assert_eq!(sha_check.result, Verifiability::Verified);

    // A CycloneDX SBOM describing weights, tokenizer, config, lineage. [NFR-OSS-1]
    let sbom = &prep.sbom;
    assert!(sbom.contains("\"bomFormat\": \"CycloneDX\""));
    assert!(sbom.contains("machine-learning-model"));
    assert!(sbom.contains("model.weights"));
    assert!(sbom.contains("tokenizer.json"));
    assert!(sbom.contains("config.json"));
    assert!(sbom.contains("midgard:declared_lineage"));
    assert!(sbom.contains(&digest)); // the verified weights digest appears in the SBOM

    // The A.9 assessment is advisory (non-OSI custom terms, no verdict). [FR-MS-6]
    assert_eq!(prep.assessment.licence.osi_approved, Tri::False);
    assert!(!prep
        .assessment
        .to_json()
        .to_ascii_lowercase()
        .contains("\"verdict\""));

    // --- Phase 2: the human gate captures the licence and re-signing occurs ---
    let captured = Licence {
        name: "Gemma Terms of Use".into(),
        osi_approved: false,
        redistribution_ok: false,
    };
    let gate = HumanLicenceGate {
        reviewer: "risk.reviewer@veldris".into(),
        captured: captured.clone(),
        accepted: true,
    };
    let (vk, sk) = veldris_keys();
    let bundle = finalize(prep, gate, &sk).expect("finalize should re-sign");

    // Licence captured into the bundle + manifest. [FR-MS-6]
    assert_eq!(bundle.licence, captured);
    assert_eq!(bundle.artifact.manifest.licence, captured);
    // Manifest is anchored to the verified weights digest.
    assert_eq!(bundle.artifact.manifest.sha256, sha256(&weights));

    // Re-signed under Veldris's ML-DSA key: the in-enclosure loader, trusting the
    // Veldris verifying key, accepts the bundle (custody transfer complete). [NFR-PQ-3]
    let loader = ModelLoader::new(vk, LicencePolicy::default());
    let loaded = loader
        .verify_and_load(&bundle.artifact, &gpu_node(80))
        .expect("re-signed artefact must verify under the Veldris key");
    assert_eq!(loaded.family, "Gemma");
    assert_eq!(loaded.id, "gemma-2-9b-1");
}

#[test]
fn ac_ms_1_unverifiable_provenance_is_recorded_but_not_fatal() {
    // No published digest, no vendor signature: provenance is Unverifiable, and
    // staging proceeds (recorded for explicit human risk-acceptance). [FR-MS-2]
    let weights = b"UNSIGNED-BLOB".to_vec();
    let fetcher = ReferenceFetcher::new().with(gemma_fixture(&weights, None, false));

    let prep = prepare(&fetcher, &gemma_source()).expect("unverifiable provenance is not fatal");
    assert!(!prep.provenance.has_mismatch());
    let sha_check = prep
        .provenance
        .checks
        .iter()
        .find(|c| c.check == "sha256-weights")
        .unwrap();
    assert_eq!(sha_check.result, Verifiability::Unverifiable);

    // It can still be re-signed once a human accepts the (higher) risk.
    let gate = HumanLicenceGate {
        reviewer: "risk.reviewer@veldris".into(),
        captured: Licence {
            name: "Gemma Terms of Use".into(),
            osi_approved: false,
            redistribution_ok: false,
        },
        accepted: true,
    };
    let (_vk, sk) = veldris_keys();
    assert!(finalize(prep, gate, &sk).is_ok());
}

#[test]
fn ac_ms_1_provenance_mismatch_fails_closed_and_never_resigns() {
    // A published digest that does NOT match the bytes ⇒ possible tampering ⇒
    // prepare fails closed; nothing is re-signed. [FR-MS-2]
    let weights = b"TAMPERED".to_vec();
    let wrong_digest = hex(&sha256(b"the-original-bytes"));
    let fetcher = ReferenceFetcher::new().with(gemma_fixture(&weights, Some(wrong_digest), true));

    match prepare(&fetcher, &gemma_source()) {
        Err(IntakeError::ProvenanceMismatch(src)) => {
            assert!(src.contains("google/gemma-2-9b"));
        }
        other => panic!("expected ProvenanceMismatch, got {other:?}"),
    }
}

#[test]
fn ac_ms_1_human_gate_rejection_halts_intake() {
    // The A.9 assessor is advisory; only the human gate approves. A rejection
    // halts intake with no re-sign. [FR-MS-6]
    let weights = b"GEMMA".to_vec();
    let digest = hex(&sha256(&weights));
    let fetcher = ReferenceFetcher::new().with(gemma_fixture(&weights, Some(digest), true));
    let prep = prepare(&fetcher, &gemma_source()).unwrap();

    let gate = HumanLicenceGate {
        reviewer: "risk.reviewer@veldris".into(),
        captured: Licence {
            name: "Gemma Terms of Use".into(),
            osi_approved: false,
            redistribution_ok: false,
        },
        accepted: false,
    };
    let (_vk, sk) = veldris_keys();
    match finalize(prep, gate, &sk) {
        Err(IntakeError::LicenceRejectedByHuman { reviewer }) => {
            assert_eq!(reviewer, "risk.reviewer@veldris");
        }
        other => panic!("expected LicenceRejectedByHuman, got {other:?}"),
    }
}
