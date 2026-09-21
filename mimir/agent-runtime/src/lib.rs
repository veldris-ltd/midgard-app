//! # agent-runtime — assembly, OAG-grounded response, and the governed loop [FR-MI-1]
//!
//! This crate hosts:
//! - the deterministic **eight-layer prompt-assembly pipeline** (BUILD_PLAYBOOK
//!   §0.1) with per-layer token budgets and the reject-don't-trim rule
//!   ([`assembly`]) — Prompt 2.c;
//! - the **OAG-grounded response** binding citations to fabric evidence
//!   ([`respond`], AC-MI-2) — Prompt 2.c;
//! - the **Mimir core agent** (A.1) and its typed output contract ([`agent`]),
//!   the **Evals** harness and A.1 starter suite ([`evals`]), and the **governed
//!   plan-act-observe loop** enforcing envelope invariants INV-1..8 ([`govloop`],
//!   AC-MI-3) — Prompt 2.d.

pub mod agent;
pub mod assembly;
pub mod builder;
pub mod defend;
pub mod evals;
pub mod govloop;
pub mod guardrail;
pub mod heal;
pub mod learn;
pub mod respond;

pub use builder::{AgentSpec, LogicBuilder, ProCodeBuilder, ToolSpec};

pub use agent::{
    ActionIntent, AgentModel, AgentOutput, AgentRefusal, ModelInput, ProposedAction,
    ReferenceAgent, RefusalCode,
};
pub use assembly::{
    AssembledPrompt, AssemblyConfig, AssemblyError, Layers, Pipeline, TokenCounter, WordCounter,
};
pub use defend::{
    Classifier, DefenseCategory, DefenseVerdict, ModelBackedGuardrail, SelfDefending, Severity,
    Verdict,
};
pub use evals::{
    ev_a1_ground, ev_a1_missing_tool_refusal, ev_a1_mutation, ev_a1_notool, ev_a1_policy,
    ev_a1_schema, ev_a1_ungrounded, ev_a2_clearance, ev_a2_nogroundneg, ev_a2_typed, ev_a3_benign,
    ev_a3_fp, ev_a3_inject, ev_a3_obfusc, ev_a4_loop, ev_a4_modeldown, ev_a4_security,
    ev_a4_transient, ev_a5_noweights, ev_a5_poison, ev_a5_regress, ev_a5_safe, per_family,
    run_ev_a2, run_ev_a3, run_ev_a4, run_ev_a5, EvalHarness, EvalResult, EvalScore, FamilyEvalCell,
    FamilyEvalReport, StarterEvals, MODEL_FAMILIES,
};
pub use govloop::{
    default_gate_resource, ActionCatalog, ActionExecutor, ActionMeta, ActionOutcome, AgentTask,
    Applied, ApproveAll, DenyAll, GovernedLoop, HaltReason, HealingRecord, HumanDecision,
    HumanGate, LoopOutcome, Preauth, StepBudget,
};
pub use guardrail::{AllowAll, GuardVerdict, InputGuardrail};
pub use heal::{
    FailureContext, FailureKind, Recovery, RecoveryAction, RecoverySupervisor, SelfHealing,
};
pub use learn::{
    DistillResult, FeedbackDistiller, FeedbackItem, LearningState, Proposal, ProposalKind,
    ProvenanceEntry, Rejected, Risk,
};
pub use respond::{render_policy, respond, GroundedResponse, Refusal, RespondConfig};

// Re-exports so a caller wires one surface.
pub use oag_retriever::{
    formulate_plan, ground, Grounding, GroundingFact, GroundingSource, OntologySummary,
    PolicyContext, RetrievalPlan,
};
