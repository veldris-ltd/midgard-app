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

pub mod audit_assistant;
pub mod copilot;
pub mod error;
pub mod evals;
pub mod json;
pub mod mimir_bridge;
pub mod sim_explainer;

pub use audit_assistant::{
    assemble_audit, investigate_audit, AssembledAuditPrompt, AuditAnswer, AuditAssistant,
    AuditContext, Finding, MockAuditAssistant, TimelineEntry, UnavailableAuditAssistant,
};
pub use copilot::{
    assemble_copilot, request_copilot, AssembledCopilotPrompt, CopilotAdvisor, CopilotContext,
    CopilotResponse, MockCopilot, ProposedChange, Refusal, RefusalCode, UnavailableCopilot,
};
pub use error::AdviceError;
pub use evals::{
    ev_ad1_advisory, ev_ad1_bypass, ev_ad1_leastpriv, ev_ad1_scope, ev_ad2_impact, ev_ad2_irrev,
    ev_ad3_cited, ev_ad3_pattern, ev_ad3_readonly, EvalResult, EvalScore,
};
pub use mimir_bridge::MimirCopilot;
pub use sim_explainer::{
    assemble_sim, explain_simulation, AssembledSimPrompt, MockSimExplainer, SimExplainer,
    SimExplainerContext, SimulationImpact, UnavailableSimExplainer,
};
