//! Convenience engine registries.
//!
//! A registry is just an ordered list of engines the node *might* run; the
//! selector picks among those that are hardware-eligible. Every engine here is
//! in-enclosure — there is no hosted entry to select. [AC-MI-1]

use std::sync::Arc;

use crate::backend::ModelBackend;
use crate::backends::{GpuDockerBackend, LocalReference};
use crate::hardware::HwProfile;

/// The vLLM OpenAI-compatible serving image (v0.11.0, Apache-2.0), pinned by digest.
pub const VLLM_IMAGE: &str =
    "vllm/vllm-openai@sha256:014a95f21c9edf6abe0aea6b07353f96baa4ec291c427bb1176dc7c93a85845c";

/// llama.cpp's `llama-server` image (MIT), pinned by the digest of its multi-arch index
/// (linux/amd64 and linux/arm64), so each host pulls its native build.
pub const LLAMA_CPP_IMAGE: &str = "ghcr.io/ggml-org/llama.cpp:server@sha256:bbf6fde2eedbe1b6c8ee9d93a54ea4606e90a4eda18681a7f8b4b6ed0cbc9f00";

/// Just the deterministic reference engine — the offline/CI floor.
pub fn reference_registry() -> Vec<Arc<dyn ModelBackend>> {
    vec![Arc::new(LocalReference::new())]
}

/// The default fleet registry: two GPU tiers over the reference floor. On a
/// large multi-GPU node the large tier is selected; on a single mid-GPU node the
/// smaller tier; on a CPU-only node the reference engine. This is the autonomous,
/// hardware-aware switching the plane performs at bootstrap.
pub fn default_registry() -> Vec<Arc<dyn ModelBackend>> {
    vec![
        Arc::new(GpuDockerBackend::vllm(
            "gpu-docker/vllm-large",
            VLLM_IMAGE,
            "/models/mistral",
            HwProfile::multi(2, 80, false),
            30,
        )),
        Arc::new(GpuDockerBackend::vllm(
            "gpu-docker/vllm-small",
            VLLM_IMAGE,
            "/models/gemma",
            HwProfile::single_gpu(24),
            20,
        )),
        Arc::new(LocalReference::new()),
    ]
}
