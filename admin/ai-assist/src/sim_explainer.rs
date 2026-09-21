//! A.AD2 — the policy-simulation explainer. Turns a raw simulation diff into a
//! plain-language impact summary, leading with the highest blast radius. It does
//! not decide and does not approve. `[FR-AD-6]`

use mimir_prompts::A_AD2_SIM_EXPLAINER;

use crate::error::AdviceError;
use crate::json::{self, string_array};

/// The inference seam for A.AD2.
pub trait SimExplainer {
    /// Produce the model's raw JSON impact summary for `prompt`, or fail closed.
    fn explain(&self, prompt: &AssembledSimPrompt) -> Result<String, AdviceError>;
}

/// A deterministic explainer returning a fixed JSON response.
pub struct MockSimExplainer {
    response: String,
}

impl MockSimExplainer {
    /// An explainer that always returns `response`.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

impl SimExplainer for MockSimExplainer {
    fn explain(&self, _prompt: &AssembledSimPrompt) -> Result<String, AdviceError> {
        Ok(self.response.clone())
    }
}

/// An explainer whose model call always fails.
pub struct UnavailableSimExplainer;

impl SimExplainer for UnavailableSimExplainer {
    fn explain(&self, _prompt: &AssembledSimPrompt) -> Result<String, AdviceError> {
        Err(AdviceError::Advisor("model server unreachable".into()))
    }
}

/// The context injected into A.AD2.
#[derive(Debug, Clone)]
pub struct SimExplainerContext {
    /// `{{SIMULATION}}` — the computed diff.
    pub simulation: String,
    /// `{{PROPOSED_CHANGE}}` — the change being explained.
    pub proposed_change: String,
}

/// The assembled A.AD2 prompt.
#[derive(Debug, Clone)]
pub struct AssembledSimPrompt {
    /// The verbatim A.AD2 system prompt.
    pub system: String,
    /// Rendered `{{SIMULATION}}`.
    pub simulation: String,
    /// Rendered `{{PROPOSED_CHANGE}}`.
    pub proposed_change: String,
}

impl AssembledSimPrompt {
    /// The full text the adapter sends to the model.
    pub fn full_text(&self) -> String {
        format!(
            "{}\n\nSIMULATION:\n{}\n\nPROPOSED_CHANGE:\n{}\n",
            self.system, self.simulation, self.proposed_change
        )
    }
}

/// Assemble the A.AD2 prompt.
pub fn assemble_sim(ctx: &SimExplainerContext) -> AssembledSimPrompt {
    AssembledSimPrompt {
        system: A_AD2_SIM_EXPLAINER.to_string(),
        simulation: ctx.simulation.clone(),
        proposed_change: ctx.proposed_change.clone(),
    }
}

/// The parsed A.AD2 impact summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationImpact {
    /// The plain-language summary (leads with the highest-impact effect).
    pub summary: String,
    /// What the change would grant.
    pub grants: Vec<String>,
    /// What the change would revoke.
    pub revokes: Vec<String>,
    /// What the change would alter.
    pub alters: Vec<String>,
    /// Whether the change is high blast radius.
    pub high_blast_radius: bool,
    /// Whether the change is irreversible.
    pub irreversible: bool,
    /// Anything surprising the model flagged.
    pub surprises: Vec<String>,
}

impl SimulationImpact {
    /// Parse an A.AD2 JSON response. Fails closed on malformed input.
    pub fn parse(text: &str) -> Result<Self, AdviceError> {
        let root = json::parse(text).map_err(AdviceError::Parse)?;
        Ok(Self {
            summary: root
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            grants: string_array(&root, "grants"),
            revokes: string_array(&root, "revokes"),
            alters: string_array(&root, "alters"),
            high_blast_radius: root
                .get("high_blast_radius")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            irreversible: root
                .get("irreversible")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            surprises: string_array(&root, "surprises"),
        })
    }
}

/// Ask the explainer (via `advisor`) to summarise a simulation. Fails closed.
pub fn explain_simulation<A: SimExplainer>(
    ctx: &SimExplainerContext,
    advisor: &A,
) -> Result<SimulationImpact, AdviceError> {
    let prompt = assemble_sim(ctx);
    let raw = advisor.explain(&prompt)?;
    SimulationImpact::parse(&raw)
}
