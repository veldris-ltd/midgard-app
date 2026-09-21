//! The serving seam: the trait every engine implements, and the descriptor the
//! autonomous selector reads.
//!
//! Per the §2.a decision, the security-critical control plane (typed API,
//! batching, KV-cache, quantisation accounting, PQ-mTLS + audit) is Rust; the
//! actual model engine sits *behind this trait* as an interchangeable backend.
//! The reference engine ([`crate::backends::LocalReference`]) proves the
//! acceptance criteria offline; the GPU engine
//! ([`crate::backends::GpuDockerBackend`]) plugs into the same seam and is
//! selected automatically wherever the hardware supports it. [FR-MI-1]

use crate::hardware::HwProfile;
use crate::quant::Quant;

/// Where an engine runs. Mimir registers **only** in-enclosure engines — a
/// hosted/remote backend is intentionally never constructed, which is what makes
/// "no hosted-model dependency" an invariant of the registry, not a hope.
/// [AC-MI-1] [DD-4]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locality {
    /// Runs inside the sovereign enclosure.
    InEnclosure,
    /// A hosted/remote model — never registered by Mimir. Present only so the
    /// wire form can round-trip and the selector can reject it if one ever
    /// appeared.
    Hosted,
}

impl Locality {
    /// Wire tag.
    pub fn as_u8(self) -> u8 {
        match self {
            Locality::InEnclosure => 0,
            Locality::Hosted => 1,
        }
    }
    /// Decode a wire tag; `None` if unknown (fail closed).
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Locality::InEnclosure,
            1 => Locality::Hosted,
            _ => return None,
        })
    }
}

/// A backend's advertised capabilities — the input to autonomous selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDescriptor {
    /// Stable engine id (e.g. "local-reference", "gpu-docker/vllm-large").
    pub id: String,
    /// Where the engine runs.
    pub locality: Locality,
    /// The hardware the node must satisfy before this engine may be placed.
    pub hw: HwProfile,
    /// Supported quantisations, in preference order (the first is the default).
    pub quants: Vec<Quant>,
    /// Capability rank; the selector prefers the highest eligible rank.
    pub rank: u8,
    /// Model families this engine can serve (Phase 3 routes on these).
    pub families: Vec<String>,
}

impl BackendDescriptor {
    /// The default quantisation (first in preference order), or full precision.
    pub fn default_quant(&self) -> Quant {
        self.quants.first().copied().unwrap_or(Quant::None)
    }
    /// Resolve a requested quant against what this engine supports, falling back
    /// to the default when the request is unsupported or absent.
    pub fn resolve_quant(&self, requested: Option<Quant>) -> Quant {
        match requested {
            Some(q) if self.quants.contains(&q) => q,
            _ => self.default_quant(),
        }
    }
}

/// A request prepared for an engine (quant resolved, budget set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRequest {
    /// The prompt to serve.
    pub prompt: String,
    /// Generation budget.
    pub max_tokens: u32,
    /// The resolved quantisation.
    pub quant: Quant,
}

/// A raw completion from an engine (before serving evidence is attached).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawCompletion {
    /// Generated text.
    pub text: String,
    /// First-token latency in milliseconds.
    pub first_token_ms: u32,
    /// Whether the prompt prefix hit the KV-cache.
    pub cache_hit: bool,
    /// Approximate prompt token count.
    pub prompt_tokens: u32,
    /// Generated token count.
    pub completion_tokens: u32,
}

/// A serving engine. `Send + Sync` so one engine is shared across the
/// thread-per-connection gateway.
pub trait ModelBackend: Send + Sync {
    /// This engine's advertised capabilities.
    fn descriptor(&self) -> &BackendDescriptor;

    /// Serve a batch of prepared requests, returning one completion per request
    /// in order. Continuous batching is the serving-plane concern
    /// ([`crate::serve`]); an engine receives already-formed batches so it can
    /// exploit them (shared KV-cache, fused kernels).
    fn infer_batch(&self, batch: &[PreparedRequest]) -> Result<Vec<RawCompletion>, InferError>;
}

/// A serving failure. All variants fail closed — no partial or hosted fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferError {
    /// No registered engine is eligible for this node.
    NoEligibleBackend,
    /// An explicitly-pinned backend is not hardware-eligible (no silent
    /// downgrade of an explicit request).
    PinnedBackendIneligible(String),
    /// The selected engine could not serve (e.g. GPU sidecar not launched).
    Unavailable(String),
}

impl std::fmt::Display for InferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferError::NoEligibleBackend => write!(f, "no eligible backend for this node"),
            InferError::PinnedBackendIneligible(id) => {
                write!(f, "pinned backend '{id}' is not hardware-eligible")
            }
            InferError::Unavailable(s) => write!(f, "backend unavailable: {s}"),
        }
    }
}

impl std::error::Error for InferError {}
