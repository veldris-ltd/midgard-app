//! No-code (Logic) and pro-code builders over the **same agent contract**.
//!
//! Both builders produce the identical [`AgentSpec`] — the declarative agent
//! definition (system prompt, least-privilege tools, reversibility, pre-authorised
//! scopes, budgets) — and both instantiate the identical governed
//! [`GovernedLoop`] from it. The **Logic builder** is a declarative, no-code
//! surface (an author fills in a form); the **pro-code builder** takes the same
//! spec and additionally lets an author swap in a custom model, evals, guardrail,
//! human gate, or supervisor. Because they share the contract, a Logic-authored
//! agent and a pro-code-authored one behave identically for the same inputs.
//! `[FR-MI-4]`

use governance::Governance;
use mimir_prompts::{A1_DEFAULT_OUTPUT_CONTRACT, A1_MIMIR_CORE_AGENT};
use policy::Resource;

use crate::agent::{ActionIntent, AgentModel};
use crate::assembly::Pipeline;
use crate::defend::SelfDefending;
use crate::evals::EvalHarness;
use crate::govloop::{
    default_gate_resource, ActionCatalog, AgentTask, GovernedLoop, HumanGate, Preauth, StepBudget,
};
use crate::guardrail::InputGuardrail;
use crate::heal::{RecoverySupervisor, SelfHealing};

/// A tool the agent may propose, with its governance metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    /// The fabric action / tool name (also the policy action name).
    pub name: String,
    /// Whether it is reversible (INV-5); irreversible tools always need a human.
    pub reversible: bool,
    /// Purposes for which it is pre-authorised (no human gate for a reversible,
    /// evaluated scope).
    pub preauth_purposes: Vec<String>,
}

/// The declarative agent definition both builders emit and the loop consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSpec {
    /// Agent name.
    pub name: String,
    /// The system prompt (defaults to A.1).
    pub system_prompt: String,
    /// The output contract (defaults to the A.1 default contract).
    pub output_contract: String,
    /// The least-privilege tool set for this agent.
    pub tools: Vec<ToolSpec>,
    /// The prompt-assembly token budget.
    pub budget_tokens: usize,
    /// The per-turn action budget (INV-8).
    pub max_actions: usize,
}

impl AgentSpec {
    /// The tool names injected each turn (INV-2 scope).
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name.clone()).collect()
    }
    /// The action catalogue (reversibility) derived from the tools.
    pub fn catalog(&self) -> ActionCatalog {
        let mut c = ActionCatalog::new();
        for t in &self.tools {
            c = c.with(&t.name, t.reversible);
        }
        c
    }
    /// The pre-authorisations derived from the tools.
    pub fn preauth(&self) -> Preauth {
        let mut p = Preauth::new();
        for t in &self.tools {
            for purpose in &t.preauth_purposes {
                p = p.allow(&t.name, purpose);
            }
        }
        p
    }
    /// Build a task for this agent (its tools + budget are applied from the spec).
    pub fn task(&self, message: impl Into<String>, intent: Option<ActionIntent>) -> AgentTask {
        AgentTask {
            message: message.into(),
            tools: self.tool_names(),
            intent,
            budget: StepBudget {
                max_actions: self.max_actions,
            },
        }
    }
}

/// The no-code, declarative Logic builder.
#[derive(Debug, Clone)]
pub struct LogicBuilder {
    spec: AgentSpec,
}

impl LogicBuilder {
    /// Start a new agent with the A.1 defaults and no tools.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            spec: AgentSpec {
                name: name.into(),
                system_prompt: A1_MIMIR_CORE_AGENT.to_string(),
                output_contract: A1_DEFAULT_OUTPUT_CONTRACT.to_string(),
                tools: Vec::new(),
                budget_tokens: 4096,
                max_actions: 4,
            },
        }
    }
    /// Grant a tool, declaring its reversibility.
    pub fn tool(mut self, name: impl Into<String>, reversible: bool) -> Self {
        self.spec.tools.push(ToolSpec {
            name: name.into(),
            reversible,
            preauth_purposes: Vec::new(),
        });
        self
    }
    /// Pre-authorise an already-granted tool for a purpose (no-op if not granted).
    pub fn preauthorise(mut self, tool: &str, purpose: &str) -> Self {
        if let Some(t) = self.spec.tools.iter_mut().find(|t| t.name == tool) {
            t.preauth_purposes.push(purpose.to_string());
        }
        self
    }
    /// Set the prompt token budget.
    pub fn budget_tokens(mut self, n: usize) -> Self {
        self.spec.budget_tokens = n;
        self
    }
    /// Set the per-turn action budget.
    pub fn max_actions(mut self, n: usize) -> Self {
        self.spec.max_actions = n;
        self
    }
    /// Emit the agent contract.
    pub fn build(self) -> AgentSpec {
        self.spec
    }
    /// Convenience: build the governed loop with platform defaults.
    pub fn build_loop(self, governance: Governance, gate_resource: Resource) -> GovernedLoop {
        ProCodeBuilder::from_spec(self.build()).build_loop(governance, gate_resource)
    }
}

/// The pro-code builder: the same [`AgentSpec`] plus programmatic hooks.
pub struct ProCodeBuilder {
    spec: AgentSpec,
    model: Option<Box<dyn AgentModel>>,
    evals: Option<Box<dyn EvalHarness>>,
    input_guardrail: Option<Box<dyn InputGuardrail>>,
    output_guardrail: Option<Box<dyn InputGuardrail>>,
    human: Option<Box<dyn HumanGate>>,
    supervisor: Option<Box<dyn RecoverySupervisor>>,
    heal_budget: u32,
}

impl ProCodeBuilder {
    /// Start from an agent contract (e.g. one a Logic builder produced).
    pub fn from_spec(spec: AgentSpec) -> Self {
        Self {
            spec,
            model: None,
            evals: None,
            input_guardrail: None,
            output_guardrail: None,
            human: None,
            supervisor: None,
            heal_budget: 3,
        }
    }
    /// The agent contract this builder is over.
    pub fn spec(&self) -> &AgentSpec {
        &self.spec
    }
    /// Swap in a custom reasoning model.
    pub fn with_model(mut self, model: Box<dyn AgentModel>) -> Self {
        self.model = Some(model);
        self
    }
    /// Swap in a custom eval harness.
    pub fn with_evals(mut self, evals: Box<dyn EvalHarness>) -> Self {
        self.evals = Some(evals);
        self
    }
    /// Swap in a custom layer-8 input guardrail (default: A.3 self-defending).
    pub fn with_input_guardrail(mut self, g: Box<dyn InputGuardrail>) -> Self {
        self.input_guardrail = Some(g);
        self
    }
    /// Set the output guardrail.
    pub fn with_output_guardrail(mut self, g: Box<dyn InputGuardrail>) -> Self {
        self.output_guardrail = Some(g);
        self
    }
    /// Set the human gate (default: deny-all, fail closed).
    pub fn with_human(mut self, human: Box<dyn HumanGate>) -> Self {
        self.human = Some(human);
        self
    }
    /// Swap in a custom recovery supervisor (default: A.4 self-healing).
    pub fn with_supervisor(mut self, s: Box<dyn RecoverySupervisor>, heal_budget: u32) -> Self {
        self.supervisor = Some(s);
        self.heal_budget = heal_budget.max(1);
        self
    }

    /// Instantiate the governed loop. Self-defending (A.3) and self-healing (A.4)
    /// are wired by default — an agent is defended and recoverable unless the
    /// author deliberately replaces them.
    pub fn build_loop(self, governance: Governance, gate_resource: Resource) -> GovernedLoop {
        let pipeline = Pipeline::new().with_guardrail(
            self.input_guardrail
                .unwrap_or_else(|| Box::new(SelfDefending::new())),
        );
        let supervisor = self
            .supervisor
            .unwrap_or_else(|| Box::new(SelfHealing::new()));

        let mut l = GovernedLoop::new(governance, gate_resource)
            .with_pipeline(pipeline)
            .with_catalog(self.spec.catalog())
            .with_preauth(self.spec.preauth())
            .with_budget_tokens(self.spec.budget_tokens)
            .with_supervisor(supervisor, self.heal_budget);
        if let Some(m) = self.model {
            l = l.with_model(m);
        }
        if let Some(e) = self.evals {
            l = l.with_evals(e);
        }
        if let Some(h) = self.human {
            l = l.with_human(h);
        }
        if let Some(og) = self.output_guardrail {
            l = l.with_output_guardrail(og);
        }
        l
    }
}

/// The default agent-gate audit resource (re-exported for builder callers).
pub fn gate_resource() -> Resource {
    default_gate_resource()
}
