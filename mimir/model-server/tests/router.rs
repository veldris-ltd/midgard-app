//! Model Abstraction Layer acceptance (SAD_02b §5, Prompt 3.c). Per-family
//! adapters + the router serve four families behind the UNCHANGED inference
//! contract.
//!
//!   AC-MS-4  four families registered → a consumer calling the unchanged API
//!            gets an eligible, policy-permitted artefact; no consumer code change.
//!   AC-MS-8  with no outbound connectivity, inference runs from locally-registered,
//!            verified artefacts (no hosted dependency).
//!   AC-MS-9  a cross-family swap is hot-swappable without contract change or data
//!            migration.
#![cfg(feature = "backend-fips")]

use model_server::api::InferenceRequest;
use model_server::backends::LocalReference;
use model_server::{
    sign_manifest, FamilyAdapter, GemmaAdapter, MistralAdapter, HwProfile, Licence, LicencePolicy,
    Locality, ModelManifest, ModelRegistry, NemotronAdapter, NodeProfile, RouteError, Router,
    RoutingPolicy, WorkloadClass,
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

fn manifest(id: &str, family: &str, hw: HwProfile) -> ModelManifest {
    ModelManifest {
        id: id.into(),
        family: family.into(),
        version: "1.0".into(),
        sha256: [7u8; 32],
        hw,
        licence: Licence {
            name: "test-licence".into(),
            osi_approved: true,
            redistribution_ok: true,
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

/// A registry with one eligible small variant of each family, plus a large
/// multi-GPU Mistral variant (to exercise degradation/eligibility).
fn family_registry() -> ModelRegistry {
    let (vk, sk) = veldris_keys();
    let mut reg = ModelRegistry::new(vk, LicencePolicy::default());
    for (id, fam) in [
        ("mistral-small", "Mistral"),
        ("nemotron-small", "Nemotron"),
        ("gemma-small", "Gemma"),
        ("granite-small", "Granite"),
    ] {
        reg.register(
            sign_manifest(manifest(id, fam, HwProfile::single_gpu(24)), &sk),
            Some("sbom".into()),
            Some("prov".into()),
        );
    }
    reg.register(
        sign_manifest(
            manifest("mistral-large", "Mistral", HwProfile::multi(2, 80, false)),
            &sk,
        ),
        None,
        None,
    );
    reg
}

/// The one family-neutral prompt a consumer sends — identical across families.
const NEUTRAL_PROMPT: &str = "SYSTEM: ground everything.\nUSER: summarise case-1.";

fn request(workload: WorkloadClass) -> InferenceRequest {
    InferenceRequest {
        purpose: "analysis".into(),
        workload_class: workload,
        quant_pref: None,
        backend_override: None,
        max_tokens: 16,
        prompt: NEUTRAL_PROMPT.into(),
    }
}

// --- AC-MS-4: unchanged contract routes to eligible, policy-permitted family --

#[test]
fn ac_ms_4_workload_routes_to_the_right_family_no_consumer_change() {
    let mut reg = family_registry();
    let mut router = Router::new(RoutingPolicy::permit_all());
    let engine = LocalReference::new();
    let node = gpu_node(1, 80); // single 80GB GPU

    // The SAME request type, differing only in the workload class the contract
    // already carries — no consumer code change per family.
    let cases = [
        (WorkloadClass::DeepReasoning, "Mistral", "mistral-small"),
        (WorkloadClass::FastTransform, "Gemma", "gemma-small"),
        (WorkloadClass::CodeAnalysis, "Nemotron", "nemotron-small"),
        (WorkloadClass::Balanced, "Mistral", "mistral-small"), // default family
    ];
    for (w, family, id) in cases {
        let out = router
            .serve(&request(w), &mut reg, &node, &engine)
            .unwrap_or_else(|e| panic!("{w:?} should route, got {e:?}"));
        assert_eq!(out.family, family, "workload {w:?}");
        assert_eq!(out.model_id, id);
        assert!(!out.text.is_empty());
    }

    // Every selection was logged (FR-MS-8).
    assert_eq!(router.decisions().len(), 4);
    assert!(router.decisions().iter().all(|d| !d.reason.is_empty()));
}

#[test]
fn ac_ms_4_family_templating_lives_in_the_adapter_not_the_prompt() {
    // The neutral prompt carries NO family chat markers …
    for marker in ["[/INST]", "<start_of_turn>", "<extra_id_1>"] {
        assert!(!NEUTRAL_PROMPT.contains(marker));
    }
    // … the adapters add them, differently per family, from the same neutral text.
    let mistral = MistralAdapter.render(NEUTRAL_PROMPT);
    let gemma = GemmaAdapter.render(NEUTRAL_PROMPT);
    let nemo = NemotronAdapter.render(NEUTRAL_PROMPT);
    assert!(mistral.starts_with("<s>[INST] ") && mistral.ends_with(" [/INST]"));
    assert!(gemma.contains("<start_of_turn>model"));
    assert!(nemo.contains("<extra_id_1>Assistant"));
    // All three wrap the identical neutral content.
    assert!(
        mistral.contains(NEUTRAL_PROMPT)
            && gemma.contains(NEUTRAL_PROMPT)
            && nemo.contains(NEUTRAL_PROMPT)
    );

    // Stop-token trimming is the adapter's job too.
    assert_eq!(
        GemmaAdapter.postprocess("hello<end_of_turn>ignored"),
        "hello"
    );
    assert_eq!(MistralAdapter.stop_tokens(), &["</s>", "[INST]"]);
    assert_eq!(MistralAdapter.postprocess(" hello </s>ignored"), "hello");
    assert_eq!(MistralAdapter.postprocess("hello[INST] next turn"), "hello");
}

#[test]
fn ac_ms_4_explicit_override_is_honoured_and_fails_closed_when_ineligible() {
    let mut reg = family_registry();
    let mut router = Router::new(RoutingPolicy::permit_all());
    let engine = LocalReference::new();
    let node = gpu_node(1, 80);

    // Pin a specific artefact via the existing contract field — routed as-is.
    let mut req = request(WorkloadClass::DeepReasoning);
    req.backend_override = Some("nemotron-small".into());
    let out = router.serve(&req, &mut reg, &node, &engine).unwrap();
    assert_eq!(out.family, "Nemotron");
    assert!(out.decision.overridden);

    // Pinning the 2-GPU large variant on a 1-GPU node fails closed (no downgrade).
    let mut big = request(WorkloadClass::DeepReasoning);
    big.backend_override = Some("mistral-large".into());
    assert_eq!(
        router.select(&big, &reg, &node),
        Err(RouteError::OverrideNotRoutable("mistral-large".into()))
    );
}

// --- AC-MS-8: no outbound connectivity — served locally & verified ------------

#[test]
fn ac_ms_8_inference_runs_locally_from_verified_registry_no_hosted_dependency() {
    let mut reg = family_registry();
    let mut router = Router::new(RoutingPolicy::permit_all());
    let engine = LocalReference::new(); // deterministic, offline; touches no network
    let node = gpu_node(1, 80);

    let out = router
        .serve(
            &request(WorkloadClass::DeepReasoning),
            &mut reg,
            &node,
            &engine,
        )
        .unwrap();

    // Ran in-enclosure, never a hosted model. [AC-MS-8]
    assert_eq!(out.locality, Locality::InEnclosure);
    // The served model is one the registry verified-on-load this call (system of
    // record proves the artefact was local + signature-verified).
    let ev = reg.load_events().last().unwrap();
    assert_eq!(ev.artefact_id, out.model_id);
    assert!(ev.sig_verified);
    assert!(ev.outcome.is_ok());
}

// --- AC-MS-9: cross-family hot swap, no contract change / data migration ------

#[test]
fn ac_ms_9_cross_family_swap_is_hot_swappable_without_contract_change() {
    let mut reg = family_registry();
    let mut router = Router::new(RoutingPolicy::permit_all());
    let engine = LocalReference::new();
    let node = gpu_node(1, 80);

    // A fixed deep-reasoning request initially routes to Mistral.
    let req = request(WorkloadClass::DeepReasoning);
    let before = router.serve(&req, &mut reg, &node, &engine).unwrap();
    assert_eq!(before.family, "Mistral");

    // Hot-swap: revoke BOTH Mistral variants at runtime (e.g. a CVE). The registry is
    // the only thing that changes — no consumer data migration, no contract change.
    reg.revoke("mistral-small");
    reg.revoke("mistral-large");

    // The IDENTICAL request now degrades to another permitted family, still Ok,
    // still the same request/response types. The deterministic candidate order is
    // alphabetical, so Gemma is picked ahead of Granite and Nemotron.
    let after = router.serve(&req, &mut reg, &node, &engine).unwrap();
    assert_ne!(after.family, "Mistral");
    assert!(["Gemma", "Granite", "Nemotron"].contains(&after.family.as_str()));
    assert_eq!(after.locality, Locality::InEnclosure);

    // Swapping a new family in is equally transparent: re-approving Mistral restores it.
    reg.set_state("mistral-small", model_server::LifecycleState::Approved);
    let restored = router.serve(&req, &mut reg, &node, &engine).unwrap();
    assert_eq!(restored.family, "Mistral");
}

#[test]
fn ac_ms_9_no_permitted_family_eligible_fails_closed() {
    let mut reg = family_registry();
    // Policy permits only a family that has no eligible artefact on a CPU node.
    let mut router = Router::new(RoutingPolicy::new(["Mistral".to_string()], "Mistral"));
    let engine = LocalReference::new();
    let cpu = NodeProfile::cpu_only(8);

    assert_eq!(
        router.serve(
            &request(WorkloadClass::DeepReasoning),
            &mut reg,
            &cpu,
            &engine
        ),
        Err(RouteError::NoEligibleArtefact)
    );
}
