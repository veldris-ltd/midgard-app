//! A.AD2 — the policy-simulation explainer. Turns a raw simulation diff into a
//! plain-language impact summary, leading with the highest blast radius. It does
//! not decide and does not approve. `[FR-AD-6]`
//!
//! The simulation diff names principals and resources other administrators wrote,
//! and the proposed change is an administrator's text, so both reach the model only
//! as provenance-tagged DATA blocks (sources `policy-simulation` and `admin`,
//! [`mimir_prompts::delimit`]) under a fresh per-request boundary, after the
//! verbatim system prompt and the data-handling rule. [VLD-ANB-MIDGARD-0080]

use mimir_prompts::delimit::{self, Boundary};
use mimir_prompts::{A_AD2_SIM_EXPLAINER, DATA_HANDLING_RULE};

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
    /// The boundary this prompt's DATA blocks carry, fresh per assembly.
    pub boundary: Boundary,
}

impl AssembledSimPrompt {
    /// The system-authority text: the verbatim A.AD2 prompt and the data-handling
    /// rule.
    pub fn system_text(&self) -> String {
        format!("{}\n\n{DATA_HANDLING_RULE}", self.system)
    }

    /// The untrusted body: the simulation diff (one bounded item per line) and the
    /// proposed change, each as a DATA block.
    pub fn body_text(&self) -> String {
        let b = self.boundary.as_str();
        let diff: Vec<&str> = self.simulation.lines().collect();
        format!(
            "SIMULATION:\n{}\n\nPROPOSED_CHANGE:\n{}",
            delimit::render_items("SIMULATION", "policy-simulation", b, &diff)
                .expect(crate::VALID_BLOCK),
            delimit::render_block("PROPOSED_CHANGE", "admin", b, &self.proposed_change)
                .expect(crate::VALID_BLOCK),
        )
    }

    /// The full text the adapter sends to the model.
    pub fn full_text(&self) -> String {
        format!("{}\n\n{}\n", self.system_text(), self.body_text())
    }
}

/// Assemble the A.AD2 prompt, under a fresh random boundary.
pub fn assemble_sim(ctx: &SimExplainerContext) -> AssembledSimPrompt {
    assemble_sim_with_boundary(ctx, crate::request_boundary())
}

/// [`assemble_sim`] under a caller-chosen boundary (tests asserting exact layout).
pub fn assemble_sim_with_boundary(
    ctx: &SimExplainerContext,
    boundary: Boundary,
) -> AssembledSimPrompt {
    AssembledSimPrompt {
        system: A_AD2_SIM_EXPLAINER.to_string(),
        simulation: ctx.simulation.clone(),
        proposed_change: ctx.proposed_change.clone(),
        boundary,
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
