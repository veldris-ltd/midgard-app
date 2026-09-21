//! A.AD1 — the admin copilot. Advisory only: it drafts **proposed** changes and
//! explains state, but never executes. To act, the administrator submits the
//! proposal through the guarded, dual-controlled path (`admin-privilege`).
//! `[FR-AD-10]` `[AC-AD-11]`

use mimir_prompts::{A_AD1_ADMIN_COPILOT, A_AD1_DEFAULT_OUTPUT_CONTRACT};

use crate::error::AdviceError;
use crate::json::{self, string_array, JsonValue};

/// The inference seam: send the assembled A.AD1 prompt to Mimir, get raw JSON back.
/// Production bridges to the model server over PQ-mTLS; tests use [`MockCopilot`].
pub trait CopilotAdvisor {
    /// Produce the model's raw JSON completion for `prompt`, or fail closed.
    fn draft(&self, prompt: &AssembledCopilotPrompt) -> Result<String, AdviceError>;
}

/// A deterministic copilot returning a fixed JSON response (the Mimir stand-in).
pub struct MockCopilot {
    response: String,
}

impl MockCopilot {
    /// A copilot that always returns `response`.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

impl CopilotAdvisor for MockCopilot {
    fn draft(&self, _prompt: &AssembledCopilotPrompt) -> Result<String, AdviceError> {
        Ok(self.response.clone())
    }
}

/// A copilot whose model call always fails — proves the panel stays usable (via the
/// guarded path) when Mimir is unreachable.
pub struct UnavailableCopilot;

impl CopilotAdvisor for UnavailableCopilot {
    fn draft(&self, _prompt: &AssembledCopilotPrompt) -> Result<String, AdviceError> {
        Err(AdviceError::Advisor("model server unreachable".into()))
    }
}

/// The runtime context injected into the A.AD1 prompt.
#[derive(Debug, Clone)]
pub struct CopilotContext {
    /// `{{ADMIN_ROLE}}` — the requester's scope.
    pub admin_role: String,
    /// `{{STATE}}` — current config/state, cited.
    pub state: String,
    /// `{{POLICY}}` — governance policy.
    pub policy: String,
    /// `{{OUTPUT_CONTRACT}}` — the JSON schema the copilot must return.
    pub output_contract: String,
    /// `{{ADMIN_MESSAGE}}` — the administrator's request.
    pub admin_message: String,
}

impl CopilotContext {
    /// A context using the default A.AD1 output contract.
    pub fn new(
        admin_role: impl Into<String>,
        state: impl Into<String>,
        policy: impl Into<String>,
        admin_message: impl Into<String>,
    ) -> Self {
        Self {
            admin_role: admin_role.into(),
            state: state.into(),
            policy: policy.into(),
            output_contract: A_AD1_DEFAULT_OUTPUT_CONTRACT.to_string(),
            admin_message: admin_message.into(),
        }
    }
}

/// The A.AD1 prompt with its injected sections rendered.
#[derive(Debug, Clone)]
pub struct AssembledCopilotPrompt {
    /// The verbatim A.AD1 system prompt.
    pub system: String,
    /// Rendered `{{ADMIN_ROLE}}`.
    pub admin_role: String,
    /// Rendered `{{STATE}}`.
    pub state: String,
    /// Rendered `{{POLICY}}`.
    pub policy: String,
    /// Rendered `{{OUTPUT_CONTRACT}}`.
    pub output_contract: String,
    /// Rendered `{{ADMIN_MESSAGE}}`.
    pub admin_message: String,
}

impl AssembledCopilotPrompt {
    /// The full text the adapter sends to the model.
    pub fn full_text(&self) -> String {
        format!(
            "{}\n\nADMIN_ROLE:\n{}\n\nSTATE:\n{}\n\nPOLICY:\n{}\n\nOUTPUT_CONTRACT:\n{}\n\nADMIN_MESSAGE:\n{}\n",
            self.system, self.admin_role, self.state, self.policy, self.output_contract, self.admin_message
        )
    }
}

/// Assemble the A.AD1 prompt for a context.
pub fn assemble_copilot(ctx: &CopilotContext) -> AssembledCopilotPrompt {
    AssembledCopilotPrompt {
        system: A_AD1_ADMIN_COPILOT.to_string(),
        admin_role: ctx.admin_role.clone(),
        state: ctx.state.clone(),
        policy: ctx.policy.clone(),
        output_contract: ctx.output_contract.clone(),
        admin_message: ctx.admin_message.clone(),
    }
}

/// A refusal code the copilot may return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalCode {
    /// The request would circumvent dual-control or audit. `[AC-AD-11]`
    ControlBypassRefused,
    /// The task exceeds the admin's role scope.
    OutOfScope,
}

impl RefusalCode {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "CONTROL_BYPASS_REFUSED" => Some(RefusalCode::ControlBypassRefused),
            "OUT_OF_SCOPE" => Some(RefusalCode::OutOfScope),
            _ => None,
        }
    }
}

/// A structured refusal (code + human detail).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The refusal code, if the copilot refused.
    pub code: Option<RefusalCode>,
    /// The refusal detail (e.g. the role that would be required).
    pub detail: Option<String>,
}

/// A single proposed change — advisory; it is executed only via the guarded path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedChange {
    /// The subsystem the change targets.
    pub subsystem: String,
    /// A description of the change.
    pub change: String,
    /// The risk level 1..=5.
    pub risk_level: u8,
    /// Whether the change will require dual-control.
    pub requires_dual_control: bool,
    /// A note on why this is the narrowest grant that meets the need.
    pub least_privilege_note: String,
    /// A suggested expiry for the grant, if applicable.
    pub suggested_expiry: Option<String>,
}

/// The parsed A.AD1 response. There is **no execute path** here — the type can only
/// express proposals, explanations, and refusals. `[AC-AD-11]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotResponse {
    /// The plain-language explanation.
    pub explanation: String,
    /// Resource citations grounding the explanation.
    pub citations: Vec<String>,
    /// The proposed changes (never executed here).
    pub proposed_changes: Vec<ProposedChange>,
    /// A refusal, if the copilot declined.
    pub refusal: Refusal,
}

impl CopilotResponse {
    /// Parse an A.AD1 JSON response. Fails closed on malformed input.
    pub fn parse(text: &str) -> Result<Self, AdviceError> {
        let root = json::parse(text).map_err(AdviceError::Parse)?;

        let explanation = root
            .get("explanation")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let citations = string_array(&root, "citations");

        let proposed_changes = match root.get("proposed_changes").and_then(|v| v.as_arr()) {
            None => Vec::new(),
            Some(items) => items.iter().map(parse_change).collect::<Result<_, _>>()?,
        };

        let refusal = match root.get("refusal") {
            None => Refusal {
                code: None,
                detail: None,
            },
            Some(r) => Refusal {
                code: r
                    .get("code")
                    .and_then(|v| v.as_str())
                    .and_then(RefusalCode::from_str),
                detail: r
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
            },
        };

        Ok(Self {
            explanation,
            citations,
            proposed_changes,
            refusal,
        })
    }
}

fn parse_change(v: &JsonValue) -> Result<ProposedChange, AdviceError> {
    let subsystem = v
        .get("subsystem")
        .and_then(|x| x.as_str())
        .ok_or_else(|| AdviceError::Parse("proposed_changes[].subsystem missing".into()))?
        .to_string();
    let change = v
        .get("change")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let risk_level = v
        .get("risk_level")
        .and_then(|x| x.as_i64())
        .filter(|n| (1..=5).contains(n))
        .ok_or_else(|| AdviceError::Parse("proposed_changes[].risk_level must be 1..=5".into()))?
        as u8;
    let requires_dual_control = v
        .get("requires_dual_control")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let least_privilege_note = v
        .get("least_privilege_note")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let suggested_expiry = v
        .get("suggested_expiry")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    Ok(ProposedChange {
        subsystem,
        change,
        risk_level,
        requires_dual_control,
        least_privilege_note,
        suggested_expiry,
    })
}

/// Ask the copilot (via `advisor`) to draft a response for `ctx`. Fails closed if
/// inference fails or the response is malformed. The result is **advisory**: any
/// proposed change must still traverse the guarded path. `[FR-AD-10]` `[AC-AD-11]`
pub fn request_copilot<A: CopilotAdvisor>(
    ctx: &CopilotContext,
    advisor: &A,
) -> Result<CopilotResponse, AdviceError> {
    let prompt = assemble_copilot(ctx);
    let raw = advisor.draft(&prompt)?;
    CopilotResponse::parse(&raw)
}
