//! Self-learning feedback distiller (A.5), offline.
//!
//! Converts governed feedback into safe, provenance-tagged **retrieval / prompt /
//! policy** updates, and **never** weight changes (AC-MI-4). The proposal kinds
//! are a closed set that structurally excludes weights ([`ProposalKind`]); poison
//! and eval-degrading feedback is rejected; PII is flagged for redaction. The A.5
//! prompt (`mimir_prompts::A5_SELF_LEARNING`) is the contract; [`FeedbackDistiller`]
//! is the deterministic reference distiller. It runs offline, out of the request
//! path. [FR-MI-5]

/// The kinds of improvement the distiller may propose. There is **no weight
/// variant** — a weight change cannot even be represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalKind {
    /// A new retrieval hint (grounding).
    RetrievalHint,
    /// A prompt clarification.
    PromptClarification,
    /// A tool-description fix.
    ToolFix,
    /// A new Evals case.
    NewEval,
}

impl ProposalKind {
    /// The contract string.
    pub fn as_str(self) -> &'static str {
        match self {
            ProposalKind::RetrievalHint => "retrieval_hint",
            ProposalKind::PromptClarification => "prompt_clarification",
            ProposalKind::ToolFix => "tool_fix",
            ProposalKind::NewEval => "new_eval",
        }
    }
}

/// Proposal risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Low.
    Low,
    /// Medium.
    Medium,
    /// High.
    High,
}

/// One governed feedback item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackItem {
    /// Provenance id.
    pub id: String,
    /// The feedback text.
    pub text: String,
}

/// A distilled improvement proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The kind of change.
    pub kind: ProposalKind,
    /// A description of the change.
    pub change: String,
    /// The feedback ids justifying it (provenance — never empty).
    pub justified_by: Vec<String>,
    /// Risk level.
    pub risk: Risk,
    /// Whether the change contains PII (forces redaction).
    pub pii_present: bool,
}

/// A rejected feedback item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// The feedback id.
    pub feedback_id: String,
    /// Why it was rejected.
    pub reason: String,
}

/// The result of distillation (mirrors the A.5 JSON).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DistillResult {
    /// Accepted proposals.
    pub proposals: Vec<Proposal>,
    /// Rejected feedback.
    pub rejected: Vec<Rejected>,
}

/// The deterministic reference distiller.
#[derive(Debug, Default, Clone, Copy)]
pub struct FeedbackDistiller;

/// Adversarial / poison markers — feedback attempting to weaken governance.
const POISON: &[&str] = &[
    "ignore the evals",
    "disable the guardrail",
    "always approve",
    "approve everything",
    "bypass policy",
    "turn off",
    "change the model weights",
    "retrain the model",
    "always allow",
];

fn contains_pii(text: &str) -> bool {
    // An email address, or a 9+ digit run (SSN/card-like).
    if text.contains('@') && text.contains('.') {
        return true;
    }
    let mut digits = 0usize;
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits += 1;
            if digits >= 9 {
                return true;
            }
        } else {
            digits = 0;
        }
    }
    false
}

fn classify_kind(lower: &str) -> ProposalKind {
    if lower.contains("retriev") || lower.contains("grounding") || lower.contains("citation") {
        ProposalKind::RetrievalHint
    } else if lower.contains("tool") {
        ProposalKind::ToolFix
    } else if lower.contains("eval") || lower.contains("test case") {
        ProposalKind::NewEval
    } else {
        // Prompt/instruction wording is the default clarification.
        ProposalKind::PromptClarification
    }
}

impl FeedbackDistiller {
    /// Build the distiller.
    pub fn new() -> Self {
        Self
    }

    /// Distil `feedback` into proposals, rejecting poison and eval-degrading
    /// items. `current_evals` are the ids of currently-passing evals that must
    /// not be weakened.
    pub fn distill(&self, feedback: &[FeedbackItem], current_evals: &[String]) -> DistillResult {
        let mut proposals = Vec::new();
        let mut rejected = Vec::new();

        for item in feedback {
            let lower = item.text.to_ascii_lowercase();

            // Reject adversarial / poison feedback.
            if POISON.iter().any(|p| lower.contains(p)) {
                rejected.push(Rejected {
                    feedback_id: item.id.clone(),
                    reason: "adversarial/poison feedback rejected".to_string(),
                });
                continue;
            }

            // Reject feedback that would degrade a currently-passing eval.
            let degrades_eval = current_evals.iter().any(|ev| {
                let e = ev.to_ascii_lowercase();
                lower.contains(&e)
                    && (lower.contains("remove")
                        || lower.contains("disable")
                        || lower.contains("skip")
                        || lower.contains("weaken"))
            });
            if degrades_eval {
                rejected.push(Rejected {
                    feedback_id: item.id.clone(),
                    reason: "would degrade a passing eval".to_string(),
                });
                continue;
            }

            let pii = contains_pii(&item.text);
            proposals.push(Proposal {
                kind: classify_kind(&lower),
                change: item.text.clone(),
                justified_by: vec![item.id.clone()],
                risk: if pii { Risk::Medium } else { Risk::Low },
                pii_present: pii,
            });
        }

        DistillResult {
            proposals,
            rejected,
        }
    }
}

/// A provenance record of an applied learning update — what changed and which
/// feedback justified it. Logged (and PQ-audited by the caller) so self-learning
/// is traceable and reversible (AC-MI-4). [NFR-AI-1]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceEntry {
    /// The kind of update applied.
    pub kind: ProposalKind,
    /// The change.
    pub change: String,
    /// Justifying feedback ids.
    pub justified_by: Vec<String>,
}

/// The accumulated learning state (retrieval hints, prompt clarifications, etc.).
/// Updates are governed: only non-PII, accepted proposals are applied, each with
/// provenance. **Model weights are never touched.**
#[derive(Debug, Default, Clone)]
pub struct LearningState {
    /// Applied updates, with provenance.
    pub applied: Vec<ProvenanceEntry>,
}

impl LearningState {
    /// A fresh state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply the non-PII proposals of a distillation, recording provenance.
    /// Returns the provenance entries written. PII-flagged proposals are held
    /// back for redaction (not applied here).
    pub fn apply(&mut self, result: &DistillResult) -> Vec<ProvenanceEntry> {
        let mut written = Vec::new();
        for p in &result.proposals {
            if p.pii_present {
                continue; // requires redaction before it may be applied
            }
            let entry = ProvenanceEntry {
                kind: p.kind,
                change: p.change.clone(),
                justified_by: p.justified_by.clone(),
            };
            self.applied.push(entry.clone());
            written.push(entry);
        }
        written
    }
}
