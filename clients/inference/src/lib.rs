//! # midgard-client-inference — automatic, audited inference tiering
//!
//! The inference adapter serves an AI request at the right **tier** — on-device
//! (Tier 1), remote enclosure Mimir (Tier 2), hybrid (Tier 3), or a graceful
//! disable — chosen per request from device capability, connectivity, and policy.
//! The choice is a runtime decision (KDD-3): the same client serves a phone and a
//! workstation. `[FR-C-6]` `[FR-C-10]`
//!
//! ## Shape (the suite's AI-adapter pattern)
//! - [`tier`] — the deterministic [`heuristic`](tier::heuristic) and the final
//!   [`guard`](tier::guard) that clamps any unsafe tier.
//! - [`reasoner`] — the advisory A.C2 seam ([`TierReasoner`]) consulted only when
//!   the heuristic is ambiguous; a deterministic guard always overrides it.
//! - [`ondevice`] — the Tier-1 assistant seam (A.C1) + its typed output contract.
//! - [`remote`] — the Tier-2 delegation seam to enclosure Mimir.
//! - [`model`] — PQ-signature verification on model load; fail closed. `[AC-C-10]`
//! - [`adapter`] — [`serve`](adapter::serve): pick a tier, dispatch, and emit an
//!   audit note; a disabled tier degrades cleanly instead of erroring.
//! - [`evals`] — EV-AC1-*, EV-AC2-*, and EV-AC-parity.
//!
//! The AI is **advisory and audited**, and the deterministic components (heuristic,
//! guard, grounding check) decide — the same discipline as every other product
//! AI touchpoint in the suite. `[NFR-AI-1]` `[KDD-1]`

#![forbid(unsafe_code)]

pub mod adapter;
pub mod error;
pub mod evals;
pub mod model;
pub mod ondevice;
pub mod prompt;
pub mod reasoner;
pub mod remote;
#[cfg(feature = "backend-fips")]
pub mod remote_bridge;
pub mod request;
pub mod tier;
pub mod wire;

pub use adapter::{
    choose_tier, seal_audit, serve, AuditNote, InferenceResult, Served, TierDecision, TierSource,
};
pub use error::InferenceError;
pub use model::{verify_and_load, SignedModel, VerifiedModel};
pub use ondevice::{MockOnDeviceRuntime, OnDeviceOutput, OnDeviceRuntime, ProposedAction, Refusal};
pub use reasoner::{FixedReasoner, PrivacyFirstReasoner, TierProposal, TierReasoner};
pub use remote::{MockRemoteInference, RemoteInference, RemoteOutput};
#[cfg(feature = "backend-fips")]
pub use remote_bridge::{PqMtlsRemoteInference, REMOTE_INFERENCE_AAD};
pub use request::InferenceRequest;
pub use tier::{
    guard, heuristic, Connectivity, DeviceCapability, Heuristic, PrivacyPreference, TaskComplexity,
    Tier, TierContext, MIN_TIER1_RAM_MB,
};
pub use wire::{decode_output, decode_request, encode_output, encode_request, WireError};
