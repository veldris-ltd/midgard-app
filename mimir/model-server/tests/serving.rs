//! FR-MI-1 serving + AC-MI-1 (local response, no hosted dependency), plus the
//! autonomous hardware-aware backend selection the plane performs at bootstrap.
//! These exercise the serving core directly — no sockets, no network, no GPU.
//! [FR-MI-1] [AC-MI-1] [NFR-PERF-1]

use std::time::Instant;

use model_server::{
    default_registry, reference_registry, select, GpuDockerBackend, HwProfile, InferenceRequest,
    LocalReference, Locality, ModelBackend, NodeProfile, Quant, Server, WorkloadClass,
    FIRST_TOKEN_BUDGET_MS,
};

fn req(prompt: &str, quant: Option<Quant>) -> InferenceRequest {
    InferenceRequest {
        purpose: "investigation".to_string(),
        workload_class: WorkloadClass::Balanced,
        quant_pref: quant,
        backend_override: None,
        max_tokens: 32,
        prompt: prompt.to_string(),
        context: None,
    }
}

/// AC-MI-1: on a node with no outbound connectivity (and here, no GPU), a request
/// is served locally, in-enclosure, with no hosted-model dependency.
#[test]
fn ac_mi_1_serves_locally_no_hosted_dependency() {
    let registry = default_registry();
    let node = NodeProfile::cpu_only(8);
    let server = Server::bootstrap(node, &registry, None).expect("bootstrap");

    // The autonomously-selected engine is the in-enclosure reference engine.
    assert_eq!(server.selection().backend_id, "local-reference");

    let prepared = server.prepare(&req("who owns case 42?", None));
    let c = server.infer(prepared).expect("served");

    assert_eq!(c.locality, Locality::InEnclosure, "must never be hosted");
    assert_eq!(c.model_id, "local-reference");
    assert!(!c.text.is_empty());
    // No engine in the registry is hosted — the invariant, not a hope.
    assert!(registry
        .iter()
        .all(|b| b.descriptor().locality == Locality::InEnclosure));
}

/// NFR-PERF-1: first-token latency is well under the 800ms budget on the local
/// path (both the modelled value and the measured wall-clock).
#[test]
fn first_token_latency_under_budget() {
    let registry = reference_registry();
    let server = Server::bootstrap(NodeProfile::cpu_only(4), &registry, None).unwrap();

    let start = Instant::now();
    let c = server
        .infer(server.prepare(&req("summarise the incident", None)))
        .unwrap();
    let elapsed_ms = start.elapsed().as_millis() as u32;

    assert!(
        c.first_token_ms < FIRST_TOKEN_BUDGET_MS,
        "modelled first-token {}ms exceeded budget",
        c.first_token_ms
    );
    assert!(
        elapsed_ms < FIRST_TOKEN_BUDGET_MS,
        "measured {elapsed_ms}ms exceeded budget"
    );
}

/// KV-cache: a repeated prompt prefix hits the cache and lowers first-token cost.
#[test]
fn kv_cache_prefix_reuse() {
    let registry = reference_registry();
    let server = Server::bootstrap(NodeProfile::cpu_only(4), &registry, None).unwrap();

    let prompt = "the quick brown fox jumps over the lazy dog again and again and again";
    let first = server.infer(server.prepare(&req(prompt, None))).unwrap();
    let second = server.infer(server.prepare(&req(prompt, None))).unwrap();

    assert!(!first.cache_hit, "first call is a cold prefix");
    assert!(
        second.cache_hit,
        "second identical prefix hits the KV-cache"
    );
    assert!(second.first_token_ms <= first.first_token_ms);
}

/// Continuous batching: N requests are served in ceil(N/max_batch) engine calls.
#[test]
fn continuous_batching_forms_batches() {
    let registry = reference_registry();
    let server = Server::bootstrap(NodeProfile::cpu_only(4), &registry, None)
        .unwrap()
        .with_max_batch(4);

    let reqs: Vec<_> = (0..10)
        .map(|i| {
            let r = req(&format!("distinct prompt number {i}"), None);
            server.prepare(&r)
        })
        .collect();

    let out = server.infer_many(reqs).unwrap();
    assert_eq!(out.len(), 10);
    // 10 requests, max batch 4 => 3 batches (4 + 4 + 2).
    assert_eq!(server.batches_formed(), 3);
    assert_eq!(out[0].batch_size, 4);
    assert_eq!(out[9].batch_size, 2);
}

/// Quantisation: an unsupported request quant falls back to a supported default;
/// a supported one is honoured.
#[test]
fn quantisation_resolution() {
    let engine = LocalReference::new();
    // The reference engine supports None/Gguf/Int8, not Int4/Awq.
    assert_eq!(
        engine.descriptor().resolve_quant(Some(Quant::Int4)),
        Quant::None
    );
    assert_eq!(
        engine.descriptor().resolve_quant(Some(Quant::Gguf)),
        Quant::Gguf
    );
    assert_eq!(engine.descriptor().resolve_quant(None), Quant::None);

    let registry = reference_registry();
    let server = Server::bootstrap(NodeProfile::cpu_only(4), &registry, None).unwrap();
    let c = server
        .infer(server.prepare(&req("hi", Some(Quant::Gguf))))
        .unwrap();
    assert_eq!(c.quant, Quant::Gguf);
}

/// Autonomous hardware-aware switching: the same registry selects a different
/// engine per node profile, and degrades gracefully with no GPU.
#[test]
fn autonomous_selection_by_hardware() {
    let registry = default_registry();

    // Large multi-GPU node -> the large GPU tier.
    let big = NodeProfile {
        gpus: 4,
        vram_gb_per_gpu: 80,
        accel: true,
        multinode: false,
        cpu_cores: 64,
        ram_gb: 512,
        in_container: true,
    };
    let (be, sel) = select(&big, &registry, None).unwrap();
    assert_eq!(be.descriptor().id, "gpu-docker/vllm-large");
    assert_eq!(sel.backend_id, "gpu-docker/vllm-large");

    // Single mid-GPU node -> the small GPU tier (large tier ineligible).
    let mid = NodeProfile {
        gpus: 1,
        vram_gb_per_gpu: 24,
        accel: true,
        multinode: false,
        cpu_cores: 16,
        ram_gb: 64,
        in_container: true,
    };
    let (be, _) = select(&mid, &registry, None).unwrap();
    assert_eq!(be.descriptor().id, "gpu-docker/vllm-small");

    // CPU-only node -> graceful degradation to the reference engine.
    let (be, _) = select(&NodeProfile::cpu_only(8), &registry, None).unwrap();
    assert_eq!(be.descriptor().id, "local-reference");
}

/// An explicit override to a hardware-ineligible engine fails closed — no silent
/// downgrade of an explicit request.
#[test]
fn override_to_ineligible_fails_closed() {
    let registry = default_registry();
    let err = Server::bootstrap(
        NodeProfile::cpu_only(8),
        &registry,
        Some("gpu-docker/vllm-large"),
    );
    assert!(
        err.is_err(),
        "pinning a GPU engine on a CPU node must fail closed"
    );
}

/// The GPU engine is a *real, inspectable* engine: its launch spec is a concrete
/// `docker run` invocation with the right GPU + quantisation flags.
#[test]
fn gpu_backend_launch_spec_is_well_formed() {
    let gpu = GpuDockerBackend::vllm(
        "gpu-docker/vllm-large",
        model_server::VLLM_IMAGE,
        "/srv/models/mistral",
        HwProfile::multi(2, 80, false),
        30,
    );
    let spec = gpu.launch_spec(Quant::Awq);
    assert_eq!(spec.program, "docker");
    assert!(spec.args.contains(&"--gpus".to_string()));
    assert!(spec.args.contains(&"all".to_string()));
    assert!(spec.args.contains(&model_server::VLLM_IMAGE.to_string()));
    assert!(model_server::VLLM_IMAGE.contains("@sha256:"));
    assert!(spec.args.contains(&"--quantization".to_string()));
    assert!(spec.args.contains(&"awq".to_string()));
    // Hardened: read-only root, no capabilities, read-only weights.
    assert!(spec.args.contains(&"--read-only".to_string()));
    assert!(spec
        .args
        .contains(&"/srv/models/mistral:/models:ro".to_string()));
    // Published only on loopback — never reachable off-host.
    assert!(spec.args.iter().any(|a| a.starts_with("127.0.0.1:")));

    // An image that is not pinned by digest is never started.
    let floating = GpuDockerBackend::vllm(
        "gpu-docker/floating",
        "vllm/vllm-openai:latest",
        "/srv/models/mistral",
        HwProfile::single_gpu(24),
        20,
    );
    assert!(floating.launch(Quant::Awq).is_err());
}

/// Hardware probe: environment overrides are honoured and parse into a profile;
/// a GPU with an explicitly-disabled accelerator does not count as accelerated.
#[test]
fn hardware_probe_from_env_overrides() {
    let vals = |k: &str| match k {
        "MIMIR_GPUS" => Some("2".to_string()),
        "MIMIR_VRAM_GB" => Some("80".to_string()),
        _ => None,
    };
    let p = NodeProfile::from_env(vals);
    assert_eq!(p.gpus, 2);
    assert_eq!(p.vram_gb_per_gpu, 80);
    assert!(p.accel, "a visible GPU implies an accelerator by default");

    let disabled = |k: &str| match k {
        "MIMIR_GPUS" => Some("1".to_string()),
        "MIMIR_ACCEL" => Some("false".to_string()),
        _ => None,
    };
    assert!(!NodeProfile::from_env(disabled).accel);
}
