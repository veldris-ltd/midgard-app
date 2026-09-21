//! The Mimir core agent (A.1) and its typed output contract.
//!
//! The agent's reasoning is a **seam** ([`AgentModel`]): the real GLM/Nemotron/
//! Gemma is served behind it (via `model-server`) in later phases; the
//! deterministic [`ReferenceAgent`] proves the governed loop offline now. Either
//! way the model produces one [`AgentOutput`] shaped exactly like the A.1 default
//! output contract, and the governed loop treats it identically. `[FR-MI-3]`
//!
//! The A.1 system prompt itself is `mimir_prompts::A1_MIMIR_CORE_AGENT` (verbatim)
//! and the contract is `mimir_prompts::A1_DEFAULT_OUTPUT_CONTRACT`.

use std::collections::BTreeMap;

use oag_retriever::Grounding;

/// A structured refusal code (from the A.1 contract's `refusal.code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalCode {
    /// A required tool was not injected for this turn.
    MissingTool,
    /// The request would exceed the caller's policy/clearance.
    PolicyDenied,
    /// The grounding does not support an answer.
    Ungrounded,
}

impl RefusalCode {
    /// The contract's string form.
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalCode::MissingTool => "MISSING_TOOL",
            RefusalCode::PolicyDenied => "POLICY_DENIED",
            RefusalCode::Ungrounded => "UNGROUNDED",
        }
    }
}

/// A structured refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRefusal {
    /// Machine-readable code.
    pub code: RefusalCode,
    /// Human-readable detail.
    pub detail: String,
}

/// A proposed state-changing action (the A.1 contract's `proposed_actions[]`).
/// The agent **proposes**; it never asserts a mutation has happened. The governed
/// loop gates and (if authorised) executes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedAction {
    /// The fabric action type (also the policy action name).
    pub action_type: String,
    /// Raw parameters (parsed against the action schema at execution).
    pub params: BTreeMap<String, String>,
    /// Why the agent proposes it.
    pub rationale: String,
    /// The grounding citations supporting the proposal.
    pub citations: Vec<String>,
}

/// The A.1 core-agent output, typed. Serialises to the default output contract.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentOutput {
    /// A short plan (≤ 6 steps).
    pub plan: Vec<String>,
    /// The answer, or `None` when refusing / proposing only.
    pub answer: Option<String>,
    /// Citations for the answer (`obj:<id>`).
    pub citations: Vec<String>,
    /// Proposed state-changing actions.
    pub proposed_actions: Vec<ProposedAction>,
    /// Labelled non-grounded reasoning.
    pub inferences: Vec<String>,
    /// A structured refusal, if any.
    pub refusal: Option<AgentRefusal>,
}

fn jstr(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            _ => o.push(c),
        }
    }
    o.push('"');
    o
}

fn jarr(items: &[String]) -> String {
    let inner = items.iter().map(|s| jstr(s)).collect::<Vec<_>>().join(",");
    format!("[{inner}]")
}

impl AgentOutput {
    /// Serialise to the A.1 default output contract shape (canonical key order).
    /// Hand-rolled (no serde), matching the suite's dependency posture.
    pub fn to_contract_json(&self) -> String {
        let answer = match &self.answer {
            Some(a) => jstr(a),
            None => "null".to_string(),
        };
        let actions = self
            .proposed_actions
            .iter()
            .map(|a| {
                let params = a
                    .params
                    .iter()
                    .map(|(k, v)| format!("{}:{}", jstr(k), jstr(v)))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"action_type\":{},\"params\":{{{}}},\"rationale\":{},\"citations\":{}}}",
                    jstr(&a.action_type),
                    params,
                    jstr(&a.rationale),
                    jarr(&a.citations)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let refusal = match &self.refusal {
            Some(r) => format!(
                "{{\"code\":{},\"detail\":{}}}",
                jstr(r.code.as_str()),
                jstr(&r.detail)
            ),
            None => "{\"code\":null,\"detail\":null}".to_string(),
        };
        format!(
            "{{\"plan\":{},\"answer\":{},\"citations\":{},\"proposed_actions\":[{}],\"inferences\":{},\"refusal\":{}}}",
            jarr(&self.plan),
            answer,
            jarr(&self.citations),
            actions,
            jarr(&self.inferences),
            refusal
        )
    }
}

/// The agent's decision to attempt a state change — in a real deployment the
/// model derives this from the task; the [`ReferenceAgent`] is handed it so the
/// governed loop can be proven deterministically. It is still subject to every
/// invariant (scope, gate, evals, audit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionIntent {
    /// The action type the agent intends to propose.
    pub action_type: String,
    /// Its parameters.
    pub params: BTreeMap<String, String>,
    /// The stated rationale.
    pub rationale: String,
}

/// Everything the model sees for one turn.
pub struct ModelInput<'a> {
    /// The fully-assembled, guardrail-passed 8-layer prompt.
    pub assembled_prompt: &'a str,
    /// The user/task message.
    pub task: &'a str,
    /// The grounding available to cite.
    pub grounding: &'a Grounding,
    /// The tool names injected for this turn (INV-2 scope).
    pub tools: &'a [String],
    /// The reference model's action decision, if any.
    pub intent: Option<ActionIntent>,
}

/// The reasoning seam. A real model is served behind this; [`ReferenceAgent`] is
/// the deterministic stand-in.
pub trait AgentModel: Send + Sync {
    /// Produce the A.1-contract output for a turn.
    fn decide(&self, input: &ModelInput) -> AgentOutput;
}

/// The deterministic reference agent. It honours the A.1 operating principles
/// structurally: it grounds answers (or refuses `UNGROUNDED`), stays in tool
/// scope (or refuses `MISSING_TOOL`), and **proposes** state changes rather than
/// asserting them.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReferenceAgent;

impl AgentModel for ReferenceAgent {
    fn decide(&self, input: &ModelInput) -> AgentOutput {
        let citations = input.grounding.citations();

        match &input.intent {
            // Action turn.
            Some(intent) => {
                let mut plan = vec![
                    "review grounded evidence".to_string(),
                    format!("propose {}", intent.action_type),
                ];
                // INV-2: never use a tool that was not injected this turn.
                if !input.tools.contains(&intent.action_type) {
                    plan.push("refuse: tool out of scope".to_string());
                    return AgentOutput {
                        plan,
                        answer: None,
                        citations: Vec::new(),
                        proposed_actions: Vec::new(),
                        inferences: Vec::new(),
                        refusal: Some(AgentRefusal {
                            code: RefusalCode::MissingTool,
                            detail: format!("tool '{}' not in scope", intent.action_type),
                        }),
                    };
                }
                AgentOutput {
                    plan,
                    answer: None,
                    citations: citations.clone(),
                    proposed_actions: vec![ProposedAction {
                        action_type: intent.action_type.clone(),
                        params: intent.params.clone(),
                        rationale: intent.rationale.clone(),
                        citations,
                    }],
                    inferences: Vec::new(),
                    refusal: None,
                }
            }
            // Question-answering turn.
            None => {
                if input.grounding.is_empty() {
                    // INV-1 / A.1 principle #1: no grounding → say so, do not guess.
                    AgentOutput {
                        plan: vec!["attempt to ground the task".to_string()],
                        answer: None,
                        citations: Vec::new(),
                        proposed_actions: Vec::new(),
                        inferences: Vec::new(),
                        refusal: Some(AgentRefusal {
                            code: RefusalCode::Ungrounded,
                            detail: "no grounding supports an answer".to_string(),
                        }),
                    }
                } else {
                    AgentOutput {
                        plan: vec![
                            "ground the task".to_string(),
                            "answer from cited evidence".to_string(),
                        ],
                        answer: Some(format!("Grounded response to: {}", input.task)),
                        citations,
                        proposed_actions: Vec::new(),
                        inferences: Vec::new(),
                        refusal: None,
                    }
                }
            }
        }
    }
}
