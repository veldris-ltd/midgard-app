//! # bifrost-risk — advisory release intelligence + a deterministic gate
//!
//! Bifrost asks Mimir (via the A.7 prompt) to score a release's risk, but the AI is
//! strictly **advisory**: a [`decide`] gate makes the go/no-go call from hard rules
//! over the [`ReleaseContext`], optionally tightened by the advisory
//! [`RiskAssessment`]. [FR-B-6] [NFR-AI-1]
//!
//! - With Mimir present, [`assess`] produces a risk score that can gate the rollout
//!   (AC-B-5).
//! - With Mimir absent, the same [`decide`] gate still enforces the hard rules —
//!   safe rollout is never contingent on the AI (AC-B-6).
//!
//! The seam is a [`RiskAdvisor`] trait (deterministic [`MockAdvisor`] for tests,
//! production bridges to the model server over PQ-mTLS), mirroring `forge/ai-assist`.

pub mod advisor;
pub mod assessment;
pub mod context;
pub mod engine;
pub mod error;
pub mod evals;
pub mod gate;
pub mod json;
pub mod mimir_bridge;
pub mod prompt;

pub use advisor::{MockAdvisor, RiskAdvisor, UnavailableAdvisor};
pub use assessment::{RiskAssessment, RiskBand, RiskFactor};
pub use context::ReleaseContext;
pub use engine::assess;
pub use error::RiskError;
pub use evals::{ev_a7_clean, ev_a7_cve, ev_a7_evidence, EvalResult, EvalScore};
pub use gate::{decide, GateDecision, RiskRules};
pub use mimir_bridge::MimirAdvisor;
pub use prompt::{assemble, AssembledPrompt};
