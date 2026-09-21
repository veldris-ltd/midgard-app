//! The governed plan-act-observe loop: the envelope that makes the eight
//! invariants (BUILD_PLAYBOOK §0.2) a contract, not a hope.
//!
//! ```text
//! ground → assemble(8-layer, INV-7 input) → model.decide
//!        → INV-7 output guardrail → INV-4 evals precheck
//!        → for each proposed action:
//!             INV-2 scoped · INV-5 reversible · INV-3 pre-act gate
//!             (human auth unless pre-authorised evaluated scope)
//!             INV-6 audit gate + execution · observe
//!        → (INV-8 step/retry budget caps the loop)
//! ```
//!
//! State-changing actions are **proposed**, evals-checked, and human-gated unless
//! pre-authorised for an evaluated, reversible scope — which is exactly AC-MI-3.
//! [NFR-AI-1]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use governance::Governance;
use mimir_prompts::{A1_DEFAULT_OUTPUT_CONTRACT, A1_MIMIR_CORE_AGENT};
use oag_retriever::{formulate_plan, ground, Grounding, GroundingSource, PolicyContext};
use policy::{ClassificationLevel, Marking, Resource, ResourceId};

use crate::agent::{AgentModel, AgentOutput, ModelInput, ProposedAction, ReferenceAgent};
use crate::assembly::{AssemblyConfig, AssemblyError, Layers, Pipeline};
use crate::evals::{EvalHarness, EvalResult, StarterEvals};
use crate::guardrail::{AllowAll, GuardVerdict, InputGuardrail};
use crate::heal::{FailureContext, FailureKind, RecoveryAction, RecoverySupervisor};
use crate::respond::render_policy;

// ---- action execution seam ------------------------------------------------

/// The outcome of applying an action through the fabric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// Whether the mutation was applied.
    pub applied: bool,
    /// Whether it was denied by policy at execution.
    pub denied: bool,
    /// The fabric audit sequence number (0 if not produced).
    pub audit_seq: u64,
}

/// Executes a gated, authorised action. Implemented for [`fabric::Fabric`], which
/// authorises + audits + runs the action internally. The loop separates *gating*
/// (its concern) from *execution* (the fabric's).
pub trait ActionExecutor {
    /// Apply an action; never called unless the loop authorised it.
    fn apply(
        &mut self,
        ctx: &PolicyContext,
        action_type: &str,
        params: &BTreeMap<String, String>,
        now: u64,
    ) -> Applied;
}

impl<S: fabric::ObjectStore, L: fabric::LedgerStore> ActionExecutor for fabric::Fabric<S, L> {
    fn apply(
        &mut self,
        ctx: &PolicyContext,
        action_type: &str,
        params: &BTreeMap<String, String>,
        now: u64,
    ) -> Applied {
        match self.apply_action(ctx, action_type, params, now) {
            Ok(r) => Applied {
                applied: r.applied,
                denied: false,
                audit_seq: r.audit_seq,
            },
            Err(fabric::FabricError::Denied) => Applied {
                applied: false,
                denied: true,
                audit_seq: 0,
            },
            Err(_) => Applied {
                applied: false,
                denied: false,
                audit_seq: 0,
            },
        }
    }
}

// ---- human gate + policy seams --------------------------------------------

/// A human authorisation decision on a proposed mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanDecision {
    /// The human authorised the action.
    Approve,
    /// The human refused it.
    Deny,
}

/// The human-in-the-loop authoriser consulted for state mutations that are not
/// pre-authorised for an evaluated scope (INV-3).
pub trait HumanGate: Send + Sync {
    /// Authorise (or refuse) one proposed action.
    fn authorize(&self, action: &ProposedAction) -> HumanDecision;
}

/// A gate that approves everything (test/automation double).
#[derive(Debug, Default, Clone, Copy)]
pub struct ApproveAll;
impl HumanGate for ApproveAll {
    fn authorize(&self, _a: &ProposedAction) -> HumanDecision {
        HumanDecision::Approve
    }
}

/// A gate that refuses everything — the fail-closed default.
#[derive(Debug, Default, Clone, Copy)]
pub struct DenyAll;
impl HumanGate for DenyAll {
    fn authorize(&self, _a: &ProposedAction) -> HumanDecision {
        HumanDecision::Deny
    }
}

/// Per-action metadata the platform declares (not the model — the model cannot be
/// trusted to self-report reversibility).
#[derive(Debug, Clone, Copy)]
pub struct ActionMeta {
    /// Whether the action is reversible (INV-5).
    pub reversible: bool,
}

/// The catalogue of action metadata. Unknown actions are treated as
/// **irreversible** (fail closed → always human-gated).
#[derive(Debug, Default, Clone)]
pub struct ActionCatalog(BTreeMap<String, ActionMeta>);

impl ActionCatalog {
    /// An empty catalogue.
    pub fn new() -> Self {
        Self::default()
    }
    /// Declare an action's reversibility.
    pub fn with(mut self, action_type: &str, reversible: bool) -> Self {
        self.0
            .insert(action_type.to_string(), ActionMeta { reversible });
        self
    }
    /// Whether an action is reversible (default false).
    pub fn reversible(&self, action_type: &str) -> bool {
        self.0
            .get(action_type)
            .map(|m| m.reversible)
            .unwrap_or(false)
    }
}

/// Pre-authorised (action_type, purpose) scopes that may execute without a human
/// gate — *provided* the action is reversible and its critical evals pass.
#[derive(Debug, Default, Clone)]
pub struct Preauth(BTreeSet<(String, String)>);

impl Preauth {
    /// No pre-authorisations (everything is human-gated).
    pub fn new() -> Self {
        Self::default()
    }
    /// Pre-authorise an action for a purpose.
    pub fn allow(mut self, action_type: &str, purpose: &str) -> Self {
        self.0
            .insert((action_type.to_string(), purpose.to_string()));
        self
    }
    /// Whether (action_type, purpose) is pre-authorised.
    pub fn is_preauthorised(&self, action_type: &str, purpose: &str) -> bool {
        self.0
            .contains(&(action_type.to_string(), purpose.to_string()))
    }
}

// ---- loop I/O -------------------------------------------------------------

/// The step/retry budget capping the loop (INV-8).
#[derive(Debug, Clone, Copy)]
pub struct StepBudget {
    /// Maximum proposed actions processed before halting to a human.
    pub max_actions: usize,
}

impl Default for StepBudget {
    fn default() -> Self {
        Self { max_actions: 4 }
    }
}

/// One agent turn's request.
pub struct AgentTask {
    /// The user/task message.
    pub message: String,
    /// The least-privilege tools injected for this turn (INV-2).
    pub tools: Vec<String>,
    /// The model's action decision, if any (see [`crate::agent::ActionIntent`]).
    pub intent: Option<crate::agent::ActionIntent>,
    /// The step budget.
    pub budget: StepBudget,
}

/// The disposition of one proposed action through the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    /// The action type.
    pub action_type: String,
    /// Whether it was evals-checked (always true — INV-4).
    pub evaluated: bool,
    /// Whether the platform considers it reversible (INV-5).
    pub reversible: bool,
    /// Whether it ran under a pre-authorised evaluated scope (no human needed).
    pub preauthorised: bool,
    /// Whether a human was consulted.
    pub human_consulted: bool,
    /// The human's decision, if consulted.
    pub human_decision: Option<HumanDecision>,
    /// Whether the mutation was actually executed.
    pub executed: bool,
    /// Whether execution was denied by policy.
    pub policy_denied: bool,
    /// The gate-decision audit sequence (agent ledger).
    pub gate_audit_seq: u64,
    /// The execution audit sequence (fabric ledger; 0 if not executed).
    pub exec_audit_seq: u64,
}

/// A self-healing recovery taken during execution (AC-MI-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealingRecord {
    /// The action being recovered.
    pub action_type: String,
    /// The recovery attempt number.
    pub attempt: u32,
    /// The recovery chosen.
    pub recovery: RecoveryAction,
    /// Whether the recovery escalated to a human.
    pub requires_human: bool,
    /// The audit sequence recording the recovery decision.
    pub audit_seq: u64,
}

/// Why the loop halted before completing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HaltReason {
    /// The non-trimmable core exceeded the token budget (assembly rejected).
    AssemblyRejected,
    /// A guardrail blocked the input or output.
    GuardrailBlocked(String),
    /// A critical eval failed.
    EvalFailed(String),
    /// The step budget was exhausted (halt to human).
    StepBudgetExhausted,
}

/// The result of one governed turn.
#[derive(Debug, Clone)]
pub struct LoopOutcome {
    /// The model's A.1 output.
    pub output: AgentOutput,
    /// The disposition of each proposed action.
    pub actions: Vec<ActionOutcome>,
    /// The eval results for the turn (INV-4).
    pub evals: Vec<EvalResult>,
    /// Why the loop halted, if it did.
    pub halted: Option<HaltReason>,
    /// Self-healing recoveries taken this turn (AC-MI-5).
    pub healing: Vec<HealingRecord>,
    /// The agent-ledger audit sequence numbers (gate decisions, halts).
    pub agent_audit_seqs: Vec<u64>,
}

impl LoopOutcome {
    /// Whether any action executed.
    pub fn executed_any(&self) -> bool {
        self.actions.iter().any(|a| a.executed)
    }
    /// Whether the agent refused.
    pub fn refused(&self) -> bool {
        self.output.refusal.is_some()
    }
}

/// The default agent-gate audit resource.
pub fn default_gate_resource() -> Resource {
    Resource {
        id: ResourceId("mimir-agent-gate".into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: BTreeSet::new(),
    }
}

// ---- the loop -------------------------------------------------------------

/// The governed agent loop.
pub struct GovernedLoop {
    model: Box<dyn AgentModel>,
    pipeline: Pipeline,
    evals: Box<dyn EvalHarness>,
    human: Box<dyn HumanGate>,
    catalog: ActionCatalog,
    preauth: Preauth,
    output_guardrail: Box<dyn InputGuardrail>,
    supervisor: Option<Box<dyn RecoverySupervisor>>,
    heal_budget: u32,
    governance: Mutex<Governance>,
    gate_resource: Resource,
    budget_tokens: usize,
}

impl GovernedLoop {
    /// Build a loop over an audit spine, with fail-closed defaults: the reference
    /// agent, the starter evals, a **deny-all** human gate, no pre-authorisations,
    /// and an empty (⇒ irreversible) action catalogue.
    pub fn new(governance: Governance, gate_resource: Resource) -> Self {
        Self {
            model: Box::new(ReferenceAgent),
            pipeline: Pipeline::new(),
            evals: Box::new(StarterEvals),
            human: Box::new(DenyAll),
            catalog: ActionCatalog::new(),
            preauth: Preauth::new(),
            output_guardrail: Box::new(AllowAll),
            supervisor: None,
            heal_budget: 3,
            governance: Mutex::new(governance),
            gate_resource,
            budget_tokens: 4096,
        }
    }

    /// Set the reasoning model.
    pub fn with_model(mut self, model: Box<dyn AgentModel>) -> Self {
        self.model = model;
        self
    }
    /// Set the human gate.
    pub fn with_human(mut self, human: Box<dyn HumanGate>) -> Self {
        self.human = human;
        self
    }
    /// Set the reasoning model.
    pub fn with_evals(mut self, evals: Box<dyn EvalHarness>) -> Self {
        self.evals = evals;
        self
    }
    /// Set the assembly pipeline (e.g. with the A.3 self-defending guardrail at
    /// layer 8).
    pub fn with_pipeline(mut self, pipeline: Pipeline) -> Self {
        self.pipeline = pipeline;
        self
    }
    /// Set the action catalogue (reversibility).
    pub fn with_catalog(mut self, catalog: ActionCatalog) -> Self {
        self.catalog = catalog;
        self
    }
    /// Set the pre-authorised scopes.
    pub fn with_preauth(mut self, preauth: Preauth) -> Self {
        self.preauth = preauth;
        self
    }
    /// Set the output guardrail.
    pub fn with_output_guardrail(mut self, g: Box<dyn InputGuardrail>) -> Self {
        self.output_guardrail = g;
        self
    }
    /// Wire the self-healing supervisor (A.4) and its retry budget. Without one,
    /// a failed execution is simply recorded as not-executed.
    pub fn with_supervisor(
        mut self,
        supervisor: Box<dyn RecoverySupervisor>,
        heal_budget: u32,
    ) -> Self {
        self.supervisor = Some(supervisor);
        self.heal_budget = heal_budget.max(1);
        self
    }
    /// Set the assembly token budget.
    pub fn with_budget_tokens(mut self, budget: usize) -> Self {
        self.budget_tokens = budget;
        self
    }

    /// The number of agent-ledger audit entries written so far.
    pub fn audit_len(&self) -> usize {
        self.governance
            .lock()
            .expect("gov poisoned")
            .audit_entries()
            .len()
    }

    fn audit(&self, ctx: &PolicyContext, action: &str, now: u64) -> u64 {
        self.governance
            .lock()
            .expect("gov poisoned")
            .authorize(ctx, &self.gate_resource, action, now)
            .map(|d| d.audit_seq)
            .unwrap_or(0)
    }

    fn audit_gate(
        &self,
        ctx: &PolicyContext,
        action_type: &str,
        authorized: bool,
        preauth: bool,
        human: bool,
        now: u64,
    ) -> u64 {
        let decision = if authorized { "AUTHORIZED" } else { "DENIED" };
        let via = if preauth {
            "preauth"
        } else if human {
            "human"
        } else {
            "auto"
        };
        self.audit(ctx, &format!("GATE:{decision}:{via}:{action_type}"), now)
    }

    /// Execute an authorised action, self-healing on execution failure (AC-MI-5).
    /// A policy denial or success returns immediately; a transient/tool failure is
    /// handed to the supervisor, which retries/reroutes within budget, or halts /
    /// escalates to a human. Returns `(executed, policy_denied, exec_audit_seq,
    /// healing_records)`.
    fn execute_with_healing<E: ActionExecutor>(
        &self,
        env: &mut E,
        ctx: &PolicyContext,
        pa: &ProposedAction,
        reversible: bool,
        base_now: u64,
    ) -> (bool, bool, u64, Vec<HealingRecord>) {
        let mut records = Vec::new();
        let mut r = env.apply(ctx, &pa.action_type, &pa.params, base_now);
        // Success, or a policy denial (not a failure to heal) — done.
        if r.applied || r.denied {
            return (r.applied, r.denied, r.audit_seq, records);
        }
        let Some(sup) = self.supervisor.as_ref() else {
            return (false, false, 0, records);
        };

        let mut attempt = 0u32;
        loop {
            let fcx = FailureContext {
                kind: FailureKind::ToolFailed,
                allowed_recoveries: vec![
                    RecoveryAction::Retry,
                    RecoveryAction::RerouteTool,
                    RecoveryAction::Rollback,
                    RecoveryAction::Halt,
                ],
                retry_budget: self.heal_budget,
                attempts: attempt,
                // A reversible action is not externally-visible for gating here.
                externally_visible: !reversible,
            };
            let rec = sup.recover(&fcx);
            let seq = self.audit(
                ctx,
                &format!("HEAL:{}:{}", rec.recovery.as_str(), pa.action_type),
                base_now + 1_000 + attempt as u64,
            );
            records.push(HealingRecord {
                action_type: pa.action_type.clone(),
                attempt,
                recovery: rec.recovery,
                requires_human: rec.requires_human,
                audit_seq: seq,
            });

            // Escalation or a non-retrying recovery stops the loop (fail closed).
            if rec.requires_human
                || matches!(
                    rec.recovery,
                    RecoveryAction::Halt | RecoveryAction::Rollback
                )
            {
                return (false, false, 0, records);
            }

            attempt += 1;
            if attempt > self.heal_budget {
                return (false, false, 0, records); // INV-8: bounded
            }
            r = env.apply(ctx, &pa.action_type, &pa.params, base_now + attempt as u64);
            if r.applied || r.denied {
                return (r.applied, r.denied, r.audit_seq, records);
            }
        }
    }

    /// Run one governed turn. `env` supplies both grounding and action execution
    /// (the fabric). Generic (not `dyn`) so it compiles on the pinned 1.82
    /// toolchain without trait upcasting.
    pub fn run<E>(
        &self,
        task: &AgentTask,
        ctx: &PolicyContext,
        env: &mut E,
        now: u64,
    ) -> LoopOutcome
    where
        E: GroundingSource + ActionExecutor,
    {
        let mut audit: Vec<u64> = Vec::new();
        let mut healing: Vec<HealingRecord> = Vec::new();

        // 1. Ground (A.2 plan → cited fabric retrieval).
        let summary = env.ontology_summary();
        let plan = formulate_plan(&task.message, &summary, &ctx.clearance);
        let grounding = if plan.groundable {
            ground(&plan, env, ctx, now)
        } else {
            Grounding::default()
        };

        // 2. Assemble the 8-layer prompt (INV-7 input guardrail is layer 8).
        let layers = Layers {
            system: A1_MIMIR_CORE_AGENT,
            output_contract: A1_DEFAULT_OUTPUT_CONTRACT,
            policy: render_policy(ctx),
            grounding: &grounding,
            memory: Vec::new(),
            tools: task.tools.clone(),
            history: Vec::new(),
            user_message: &task.message,
        };
        let assembled = match self.pipeline.assemble(
            &layers,
            &AssemblyConfig {
                budget_tokens: self.budget_tokens,
            },
        ) {
            Ok(a) => a,
            Err(e) => {
                let (halted, tag) = match e {
                    AssemblyError::CoreExceedsBudget { .. } => (
                        HaltReason::AssemblyRejected,
                        "HALT:assembly-rejected".to_string(),
                    ),
                    AssemblyError::Blocked { reason } => (
                        HaltReason::GuardrailBlocked(reason),
                        "HALT:input-guardrail".to_string(),
                    ),
                };
                audit.push(self.audit(ctx, &tag, now));
                return LoopOutcome {
                    output: AgentOutput::default(),
                    actions: Vec::new(),
                    evals: Vec::new(),
                    halted: Some(halted),
                    healing,
                    agent_audit_seqs: audit,
                };
            }
        };

        // 3. Model decides (A.1 contract output).
        let input = ModelInput {
            assembled_prompt: &assembled.text,
            task: &task.message,
            grounding: &grounding,
            tools: &task.tools,
            intent: task.intent.clone(),
        };
        let output = self.model.decide(&input);

        // 4. INV-7 output guardrail.
        let out_text = format!(
            "{}\n{}",
            output.answer.clone().unwrap_or_default(),
            output.inferences.join("\n")
        );
        if let GuardVerdict::Block { reason } = self.output_guardrail.inspect(&out_text) {
            audit.push(self.audit(ctx, "HALT:output-guardrail", now));
            return LoopOutcome {
                output,
                actions: Vec::new(),
                evals: Vec::new(),
                halted: Some(HaltReason::GuardrailBlocked(reason)),
                healing,
                agent_audit_seqs: audit,
            };
        }

        // 5. INV-4 evals precheck; a failing critical eval halts.
        let evals = self.evals.precheck(&output, &grounding, &task.tools);
        if let Some(f) = evals.iter().find(|e| e.critical && !e.passed()) {
            audit.push(self.audit(ctx, &format!("HALT:eval:{}", f.id), now));
            let id = f.id.clone();
            return LoopOutcome {
                output,
                actions: Vec::new(),
                evals,
                halted: Some(HaltReason::EvalFailed(id)),
                healing,
                agent_audit_seqs: audit,
            };
        }

        // A valid refusal is a terminal outcome (no actions to gate).
        if output.refusal.is_some() {
            audit.push(self.audit(ctx, "REFUSAL", now));
            return LoopOutcome {
                output,
                actions: Vec::new(),
                evals,
                halted: None,
                healing,
                agent_audit_seqs: audit,
            };
        }

        // INV-8: bound the loop; too many proposals halts to a human.
        if output.proposed_actions.len() > task.budget.max_actions {
            audit.push(self.audit(ctx, "HALT:step-budget", now));
            return LoopOutcome {
                output,
                actions: Vec::new(),
                evals,
                halted: Some(HaltReason::StepBudgetExhausted),
                healing,
                agent_audit_seqs: audit,
            };
        }

        // 6. Gate + execute each proposed action.
        let mut actions = Vec::new();
        let mut tick = now;
        for pa in &output.proposed_actions {
            tick += 1;

            // INV-5: reversibility is a platform fact, not the model's claim.
            let reversible = self.catalog.reversible(&pa.action_type);
            // INV-3: pre-authorised only if reversible AND scope-allowed; else a
            // human must sign off (irreversible ⇒ always human).
            let preauthorised = reversible
                && self
                    .preauth
                    .is_preauthorised(&pa.action_type, &ctx.purpose.0);
            let needs_human = !preauthorised;

            let (human_consulted, human_decision, authorized) = if needs_human {
                let d = self.human.authorize(pa);
                (true, Some(d), d == HumanDecision::Approve)
            } else {
                (false, None, true)
            };

            // INV-6: audit the gate decision (agent ledger).
            let gate_seq = self.audit_gate(
                ctx,
                &pa.action_type,
                authorized,
                preauthorised,
                human_consulted,
                tick,
            );
            audit.push(gate_seq);

            // INV-3: execute only if authorised. The fabric authorises + audits
            // the execution itself (INV-6 for the mutation).
            let (executed, policy_denied, exec_seq) = if authorized {
                let (ex, denied, seq, mut heals) =
                    self.execute_with_healing(env, ctx, pa, reversible, tick);
                healing.append(&mut heals);
                (ex, denied, seq)
            } else {
                (false, false, 0)
            };

            actions.push(ActionOutcome {
                action_type: pa.action_type.clone(),
                evaluated: true,
                reversible,
                preauthorised,
                human_consulted,
                human_decision,
                executed,
                policy_denied,
                gate_audit_seq: gate_seq,
                exec_audit_seq: exec_seq,
            });
        }

        LoopOutcome {
            output,
            actions,
            evals,
            halted: None,
            healing,
            agent_audit_seqs: audit,
        }
    }
}
