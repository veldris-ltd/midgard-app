//! # model-server — Mimir's local inference plane
//!
//! Sovereign, in-enclosure model serving behind a single typed API, exposed to
//! the rest of the suite **only** over PQ-mTLS and **audited on every call**.
//! This is FR-MI-1 (serving: batching, KV-cache, quantisation, a typed API) and
//! FR-MI-8 (PQ-mTLS-only exposure, audited) of the Mimir SAD. [FR-MI-1] [FR-MI-8]
//!
//! ## Shape (the §2.a decision)
//! The security-critical control plane is lean-std Rust; the actual model engine
//! is an interchangeable [`backend::ModelBackend`] chosen **autonomously from the
//! node's hardware** at bootstrap:
//! - [`backends::LocalReference`] — the deterministic in-enclosure engine that
//!   proves the acceptance criteria offline, with no GPU and no network.
//! - [`backends::GpuDockerBackend`] — the GPU-in-Docker engine (vLLM class),
//!   selected automatically wherever the node has the accelerators to serve it,
//!   degrading to a smaller tier or the reference engine where it does not.
//!
//! Nothing above the [`backend::ModelBackend`] seam changes when a GPU engine is
//! placed, so the inference contract is stable for Phase 3's multi-family router.
//!
//! ## Flow of one call
//! `consumer --PQ-mTLS--> gateway (auth+authz channel) --> InferenceHandler
//!  (authorize+audit) --> Server (select→batch→KV-cache→engine) --> response`.
//!
//! Every call writes one PQ-signed audit entry through the governance spine; a
//! denied or malformed call is refused **fail-closed** and still audited.

pub mod adapter;
pub mod api;
pub mod artifact;
pub mod backend;
pub mod backends;
pub mod handler;
pub mod hardware;
// The Model Registry (artefact system of record) is built on the ML-DSA
// verify-on-load gate, so it shares ModelLoader's `backend-fips` gating.
#[cfg(feature = "backend-fips")]
pub mod model_registry;
pub mod quant;
pub mod registry;
// The router serves from the (fail-closed, verified) Model Registry, so it too is
// gated on `backend-fips`.
#[cfg(feature = "backend-fips")]
pub mod router;
pub mod select;
pub mod serve;

pub use adapter::{
    AdapterSet, FamilyAdapter, GemmaAdapter, MistralAdapter, GraniteAdapter, NemotronAdapter,
};
pub use api::{Completion, InferenceRequest, InferenceResponse, Malformed, WorkloadClass};
pub use artifact::{
    digest_weights, parse_manifest_file, parse_trust_key, render_manifest_file, verify_weights,
    Licence, LicencePolicy, LoadError, LoadedModel, ModelManifest, SignedArtifact, WeightFormat,
    WeightsDigest, WeightsFile, MANIFEST_FILE, MANIFEST_FILE_HEADER, VELDRIS_MODEL_SIG_ALG,
};
#[cfg(feature = "backend-fips")]
pub use artifact::{sign_manifest, ModelLoader, VerifiedModel};
pub use backend::{
    BackendDescriptor, InferError, Locality, ModelBackend, PreparedRequest, RawCompletion,
};
pub use backends::{Engine, GpuDockerBackend, LaunchSpec, LocalReference, SidecarBackend};
pub use handler::{AuditView, Clock, InferenceHandler, LogicalClock, ServeStat, RECENT_CALLS};
pub use hardware::{probe, HwProfile, NodeProfile};
#[cfg(feature = "backend-fips")]
pub use model_registry::{LifecycleState, LoadEvent, ModelRegistry, RegisteredArtefact};
pub use quant::Quant;
pub use registry::{default_registry, reference_registry, LLAMA_CPP_IMAGE, VLLM_IMAGE};
#[cfg(feature = "backend-fips")]
pub use router::{RouteError, RoutedCompletion, Router, RoutingDecision, RoutingPolicy};
pub use select::{select, Selection};
pub use serve::{Server, DEFAULT_MAX_BATCH, FIRST_TOKEN_BUDGET_MS};
