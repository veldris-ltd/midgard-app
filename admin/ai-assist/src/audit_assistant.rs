//! A.AD3 — the audit-investigation assistant. Strictly read-only Q&A over the audit
//! ledger; every finding is cited by audit entry id, and the response type cannot
//! express any mutation. `[FR-AD-9]`

use mimir_prompts::A_AD3_AUDIT_ASSISTANT;

use crate::error::AdviceError;
use crate::json::{self, string_array, JsonValue};

/// The inference seam for A.AD3.
pub trait AuditAssistant {
    /// Produce the model's raw JSON answer for `prompt`, or fail closed.
    fn investigate(&self, prompt: &AssembledAuditPrompt) -> Result<String, AdviceError>;
}

/// A deterministic assistant returning a fixed JSON response.
pub struct MockAuditAssistant {
    response: String,
}

impl MockAuditAssistant {
    /// An assistant that always returns `response`.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

impl AuditAssistant for MockAuditAssistant {
    fn investigate(&self, _prompt: &AssembledAuditPrompt) -> Result<String, AdviceError> {
        Ok(self.response.clone())
    }
}

/// An assistant whose model call always fails.
pub struct UnavailableAuditAssistant;

impl AuditAssistant for UnavailableAuditAssistant {
    fn investigate(&self, _prompt: &AssembledAuditPrompt) -> Result<String, AdviceError> {
        Err(AdviceError::Advisor("model server unreachable".into()))
    }
}

/// The context injected into A.AD3.
#[derive(Debug, Clone)]
pub struct AuditContext {
    /// `{{AUDIT_RESULTS}}` — the scoped ledger query results.
    pub audit_results: String,
    /// `{{QUESTION}}` — the auditor's question.
    pub question: String,
}

/// The assembled A.AD3 prompt.
#[derive(Debug, Clone)]
pub struct AssembledAuditPrompt {
    /// The verbatim A.AD3 system prompt.
    pub system: String,
    /// Rendered `{{AUDIT_RESULTS}}`.
    pub audit_results: String,
    /// Rendered `{{QUESTION}}`.
    pub question: String,
}

impl AssembledAuditPrompt {
    /// The full text the adapter sends to the model.
    pub fn full_text(&self) -> String {
        format!(
            "{}\n\nAUDIT_RESULTS:\n{}\n\nQUESTION:\n{}\n",
            self.system, self.audit_results, self.question
        )
    }
}

/// Assemble the A.AD3 prompt.
pub fn assemble_audit(ctx: &AuditContext) -> AssembledAuditPrompt {
    AssembledAuditPrompt {
        system: A_AD3_AUDIT_ASSISTANT.to_string(),
        audit_results: ctx.audit_results.clone(),
        question: ctx.question.clone(),
    }
}

/// A cited finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The factual statement.
    pub statement: String,
    /// The audit entry ids backing it (e.g. `audit:<id>`).
    pub citations: Vec<String>,
}

/// One timeline row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    /// Timestamp.
    pub ts: String,
    /// Actor.
    pub actor: String,
    /// Action.
    pub action: String,
    /// The backing audit id.
    pub audit_id: String,
}

/// The parsed A.AD3 answer. Observation-only: there is no field through which the
/// assistant could propose or perform a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditAnswer {
    /// The cited findings.
    pub findings: Vec<Finding>,
    /// The reconstructed timeline.
    pub timeline: Vec<TimelineEntry>,
    /// Patterns worth human review (surfaced neutrally, without asserting intent).
    pub patterns_for_review: Vec<String>,
    /// Whether the records were insufficient to answer.
    pub insufficient_data: bool,
}

impl AuditAnswer {
    /// Parse an A.AD3 JSON response. Fails closed on malformed input.
    pub fn parse(text: &str) -> Result<Self, AdviceError> {
        let root = json::parse(text).map_err(AdviceError::Parse)?;

        let findings = match root.get("findings").and_then(|v| v.as_arr()) {
            None => Vec::new(),
            Some(items) => items.iter().map(parse_finding).collect::<Result<_, _>>()?,
        };
        let timeline = match root.get("timeline").and_then(|v| v.as_arr()) {
            None => Vec::new(),
            Some(items) => items.iter().map(parse_timeline).collect(),
        };
        Ok(Self {
            findings,
            timeline,
            patterns_for_review: string_array(&root, "patterns_for_review"),
            insufficient_data: root
                .get("insufficient_data")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
    }
}

fn parse_finding(v: &JsonValue) -> Result<Finding, AdviceError> {
    let statement = v
        .get("statement")
        .and_then(|x| x.as_str())
        .ok_or_else(|| AdviceError::Parse("findings[].statement missing".into()))?
        .to_string();
    Ok(Finding {
        statement,
        citations: string_array(v, "citations"),
    })
}

fn parse_timeline(v: &JsonValue) -> TimelineEntry {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    TimelineEntry {
        ts: s("ts"),
        actor: s("actor"),
        action: s("action"),
        audit_id: s("audit_id"),
    }
}

/// Ask the assistant (via `advisor`) to answer an audit question. Fails closed.
pub fn investigate_audit<A: AuditAssistant>(
    ctx: &AuditContext,
    advisor: &A,
) -> Result<AuditAnswer, AdviceError> {
    let prompt = assemble_audit(ctx);
    let raw = advisor.investigate(&prompt)?;
    AuditAnswer::parse(&raw)
}
