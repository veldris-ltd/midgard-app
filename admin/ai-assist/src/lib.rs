//! # admin-ai-assist — the admin panel's three advisory AI touchpoints
//!
//! All three are **advisory**: they draft, explain, and summarise, but none can
//! execute an admin action. Every real change still traverses the guarded,
//! dual-controlled path (`admin-privilege`), where policy and audit apply.
//! `[FR-AD-10]` `[NFR-AI-1]`
//!
//! - **A.AD1 admin copilot** ([`copilot`]) — drafts *proposed* changes and explains
//!   state; it never executes, refuses control-bypass requests, respects the
//!   admin's role, and prefers least privilege. `[AC-AD-11]`
//! - **A.AD2 policy-simulation explainer** ([`sim_explainer`]) — turns a raw
//!   simulation diff into a plain-language impact summary, leading with the
//!   highest blast radius. `[FR-AD-6]`
//! - **A.AD3 audit-investigation assistant** ([`audit_assistant`]) — strictly
//!   read-only Q&A over the audit ledger, every finding cited by entry id.
//!   `[FR-AD-9]`
//!
//! Each follows the suite's AI-adapter pattern (as in `bifrost-risk` /
//! `forge-ai-assist`): the verbatim system prompt lives in `mimir-prompts`, an
//! inference **seam trait** abstracts the Mimir call (production bridges over
//! PQ-mTLS; tests use a deterministic mock), the model's JSON is parsed by a small
//! dependency-free reader, and an **EV-AD** eval suite scores the behaviour — each
//! eval both passing on correct output and catching the corresponding violation.
//!
//! A.AD1, A.AD2 and A.AD3 place untrusted text (state, simulation diffs, ledger
//! rows, the administrator's words) in their prompts only as provenance-tagged DATA blocks
//! (`mimir_prompts::delimit`), each prompt under a fresh boundary from
//! [`request_boundary`]. [VLD-ANB-MIDGARD-0080] [FR-MI-7]

pub mod audit_assistant;
pub mod copilot;
pub mod error;
pub mod evals;
pub mod json;
pub mod mimir_bridge;
pub mod sim_explainer;

pub use audit_assistant::{
    assemble_audit, assemble_audit_with_boundary, investigate_audit, AssembledAuditPrompt,
    AuditAnswer, AuditAssistant, AuditContext, Finding, MockAuditAssistant, TimelineEntry,
    UnavailableAuditAssistant,
};
pub use copilot::{
    assemble_copilot, assemble_copilot_with_boundary, request_copilot, AssembledCopilotPrompt,
    CopilotAdvisor, CopilotContext, CopilotResponse, MockCopilot, ProposedChange, Refusal,
    RefusalCode, UnavailableCopilot,
};
pub use error::AdviceError;
pub use evals::{
    ev_ad1_advisory, ev_ad1_bypass, ev_ad1_leastpriv, ev_ad1_scope, ev_ad2_impact, ev_ad2_irrev,
    ev_ad3_cited, ev_ad3_pattern, ev_ad3_readonly, EvalResult, EvalScore,
};
pub use mimir_bridge::MimirCopilot;

use mimir_prompts::delimit::Boundary;
use pqc_agility::{OsRng, Rng};

/// Labels and sources are constants and a [`Boundary`] is always a token, so a
/// prompt's DATA block cannot be refused.
const VALID_BLOCK: &str = "constant label and source, and a Boundary is a token";

/// A fresh random DATA boundary for one prompt, from `pqc_agility::OsRng`.
/// [VLD-ANB-MIDGARD-0080]
pub fn request_boundary() -> Boundary {
    let mut bytes = [0u8; 8];
    OsRng.fill(&mut bytes);
    Boundary::from_random_bytes(bytes)
}
pub use sim_explainer::{
    assemble_sim, assemble_sim_with_boundary, explain_simulation, AssembledSimPrompt,
    MockSimExplainer, SimExplainer, SimExplainerContext, SimulationImpact, UnavailableSimExplainer,
};
