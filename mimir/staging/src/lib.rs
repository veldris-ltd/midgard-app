//! # staging — Mimir's model-supply staging enclave (intake side)
//!
//! The staging enclave is the **network-connected, isolated** environment that
//! fetches candidate model variants from untrusted upstreams (Hugging Face,
//! NVIDIA NGC, vendor CDNs), verifies them, licences them, SBOM-s them, and
//! **re-signs them under Veldris's ML-DSA key** so the air-gapped enclosure can
//! trust them by trusting only the Veldris signing root (custody transfer). This
//! crate implements Prompt 3.a of SAD_02b: the intake pipeline up to and
//! including re-signing. [FR-MS-1] [FR-MS-2] [FR-MS-6] [NFR-PQ-3]
//!
//! ## Isolation posture
//! Fetching untrusted upstream content is the one privileged-adjacent act in the
//! suite, so it is quarantined here: this crate has **no enclosure credentials**
//! (it does not depend on the gateway, PQ-mTLS mesh, or governance spine). The
//! only thing it produces that crosses the air gap is a signed [`StagedBundle`],
//! and Bifrost re-verifies the Veldris signature on import (Prompt 3.b / AC-MS-3).
//! A staging compromise therefore cannot reach into the enclosure. [NFR-SEC-1]
//!
//! ## The pipeline (AC-MS-1)
//! `fetch (staging only) → verify native provenance (record verifiability) →
//!  CycloneDX SBOM → advisory A.9 licence/risk assessment → human licence gate →
//!  re-sign under Veldris ML-DSA`.
//!
//! - [`fetch`] — the acquisition seam ([`Fetcher`]) + an offline reference fetcher.
//! - [`provenance`] — native-provenance verification with per-check verifiability.
//! - [`sbom`] — deterministic CycloneDX 1.5 model SBOM.
//! - [`assessor`] — the A.9 assessor: **advisory**, marks unknowns, never decides.
//! - [`evals`] — the EV-A9-* suite scoring the assessor.
//! - [`intake`] — [`prepare`] / [`finalize`] tying it together with the human gate.

pub mod assessor;
pub mod evals;
pub mod fetch;
pub mod intake;
pub mod provenance;
pub mod sbom;

pub use assessor::{AssessedLicence, IntakeAssessment, ModelIntakeAssessor, Tri};
pub use evals::{
    contains_verdict, ev_a9_noverdict, ev_a9_restrict, ev_a9_unknown, EvalResult, EvalScore,
};
pub use fetch::{
    DeclaredProvenance, FetchError, FetchedModel, Fetcher, ReferenceFetcher, UpstreamSource,
};
pub use intake::{prepare, HumanLicenceGate, IntakeError, IntakePrep, StagedBundle};
pub use provenance::{verify_provenance, ProvenanceCheck, ProvenanceRecord, Verifiability};
pub use sbom::generate_sbom;

#[cfg(feature = "backend-fips")]
pub use intake::finalize;
