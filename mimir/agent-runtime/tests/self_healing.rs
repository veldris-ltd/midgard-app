//! AC-MI-5: on a failed tool / degraded execution, the runtime retries / reroutes
//! / rolls back automatically within budget, and escalates to a human on security
//! events (self-healing, A.4). Proven at the supervisor level (EV-A4-*) and
//! end-to-end: a flaky executor is recovered by the governed loop. [FR-MI-6]

use std::collections::{BTreeMap, BTreeSet};

use agent_runtime::{
    default_gate_resource, run_ev_a4, ActionCatalog, ActionExecutor, ActionIntent, AgentTask,
    Applied, DenyAll, GovernedLoop, Preauth, RecoveryAction, SelfHealing, StepBudget,
};
use fabric::{
    ActionFunction, ActionType, Effect, Fabric, FabricError, FunctionRuntime, Governance,
    InMemoryStore, ObjectInstance, ObjectStore, ObjectType, PolicyContext, PropertyType, Query,
    TypeRegistry, Value, ValueType,
};
use oag_retriever::{GroundingSource, OntologySummary, Retrieved};
use policy::{
    Action, ClassificationLevel, Clearance, DefaultPdp, Effect as PEffect, Marking, PolicySet,
    Purpose, Role, Rule, RuleId, Selector,
};
use pqc_agility::Rng;

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let b = z.to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&b[..take]);
            i += take;
        }
    }
}

struct TriageCase;
impl ActionFunction for TriageCase {
    fn apply(
        &self,
        params: &BTreeMap<String, Value>,
        store: &dyn ObjectStore,
    ) -> Result<Vec<Effect>, FabricError> {
        let case_id = match params.get("case_id") {
            Some(Value::Text(s)) => s.clone(),
            _ => return Err(FabricError::MissingRequiredParam("case_id".into())),
        };
        let existing = store
            .get_object(&case_id)
            .ok_or_else(|| FabricError::UnknownObjectType(case_id.clone()))?;
        let mut updated = existing.clone();
        updated
            .properties
            .insert("status".into(), Value::Text("triaged".into()));
        Ok(vec![Effect::PutObject(updated)])
    }
}

fn build_fabric() -> Fabric<InMemoryStore> {
    let mut reg = TypeRegistry::new();
    reg.register_object_type(ObjectType {
        id: "case".into(),
        api_name: "Case".into(),
        title: "Case".into(),
        properties: vec![
            PropertyType::new("name", ValueType::Text, true),
            PropertyType::new("status", ValueType::Text, false),
        ],
        interfaces: vec![],
    })
    .unwrap();
    reg.register_action_type(ActionType {
        id: "triage".into(),
        api_name: "triage".into(),
        parameters: vec![PropertyType::new("case_id", ValueType::Text, true)],
        function_id: "fn_triage".into(),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    })
    .unwrap();
    let mut runtime = FunctionRuntime::new();
    runtime.register("fn_triage", Box::new(TriageCase));
    let gov = Governance::new(
        DefaultPdp::new(
            PolicySet::new(vec![
                Rule {
                    id: RuleId("read".into()),
                    role: Role("analyst".into()),
                    action: Action("Read".into()),
                    purpose: None,
                    selector: Selector::Any,
                    effect: PEffect::Permit,
                },
                Rule {
                    id: RuleId("triage".into()),
                    role: Role("analyst".into()),
                    action: Action("triage".into()),
                    purpose: None,
                    selector: Selector::Any,
                    effect: PEffect::Permit,
                },
            ])
            .unwrap(),
        ),
        &mut SeedRng(5),
    )
    .unwrap();
    let mut fab = Fabric::new(reg, InMemoryStore::new(), runtime, gov);
    let mut props = BTreeMap::new();
    props.insert("name".into(), Value::Text("Case".into()));
    props.insert("status".into(), Value::Text("open".into()));
    fab.store_mut().put_object(ObjectInstance {
        id: "case-1".into(),
        type_id: "case".into(),
        properties: props,
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    });
    fab
}

/// A fabric wrapper whose action execution fails `fails_left` times before it
/// succeeds — a transient tool failure the supervisor must recover from.
struct FlakyEnv {
    fab: Fabric<InMemoryStore>,
    fails_left: u32,
}

impl GroundingSource for FlakyEnv {
    fn ontology_summary(&self) -> OntologySummary {
        self.fab.ontology_summary()
    }
    fn retrieve(&mut self, ctx: &PolicyContext, q: &Query, now: u64) -> Result<Retrieved, String> {
        self.fab.retrieve(ctx, q, now)
    }
}

impl ActionExecutor for FlakyEnv {
    fn apply(
        &mut self,
        ctx: &PolicyContext,
        action_type: &str,
        params: &BTreeMap<String, String>,
        now: u64,
    ) -> Applied {
        if self.fails_left > 0 {
            self.fails_left -= 1;
            return Applied {
                applied: false,
                denied: false,
                audit_seq: 0,
            }; // transient execution failure
        }
        self.fab.apply(ctx, action_type, params, now)
    }
}

fn ctx() -> PolicyContext {
    PolicyContext {
        subject: "alice".into(),
        roles: [Role("analyst".into())].into_iter().collect(),
        clearance: Clearance {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        purpose: Purpose("investigation".into()),
    }
}

fn task() -> AgentTask {
    AgentTask {
        message: "triage the case".into(),
        tools: vec!["triage".into()],
        intent: Some(ActionIntent {
            action_type: "triage".into(),
            params: BTreeMap::from([("case_id".to_string(), "case-1".to_string())]),
            rationale: "needs triage".into(),
        }),
        budget: StepBudget::default(),
    }
}

/// Pre-authorise the reversible triage so no human gate is needed; the failure we
/// exercise is at execution, which is where self-healing kicks in.
fn healing_loop(budget: u32) -> GovernedLoop {
    let gov = Governance::new(
        DefaultPdp::new(PolicySet::new(vec![]).unwrap()),
        &mut SeedRng(9),
    )
    .unwrap();
    GovernedLoop::new(gov, default_gate_resource())
        .with_human(Box::new(DenyAll))
        .with_catalog(ActionCatalog::new().with("triage", true))
        .with_preauth(Preauth::new().allow("triage", "investigation"))
        .with_supervisor(Box::new(SelfHealing::new()), budget)
}

#[test]
fn ac_mi_5_transient_failure_is_retried_and_recovers() {
    let mut env = FlakyEnv {
        fab: build_fabric(),
        fails_left: 2,
    };
    let l = healing_loop(3);
    let out = l.run(&task(), &ctx(), &mut env, 100);

    let a = &out.actions[0];
    assert!(a.executed, "the action recovered and applied after retries");
    assert_eq!(out.healing.len(), 2, "two transient failures, two retries");
    assert!(out
        .healing
        .iter()
        .all(|h| h.recovery == RecoveryAction::Retry));
    // The mutation eventually took effect.
    let status = env
        .fab
        .store()
        .get_object("case-1")
        .and_then(|o| o.properties.get("status"))
        .cloned();
    assert_eq!(status, Some(Value::Text("triaged".into())));
}

#[test]
fn healing_is_bounded_by_budget() {
    // More failures than the retry budget → the loop stops trying (INV-8).
    let mut env = FlakyEnv {
        fab: build_fabric(),
        fails_left: 10,
    };
    let l = healing_loop(2);
    let out = l.run(&task(), &ctx(), &mut env, 200);

    let a = &out.actions[0];
    assert!(
        !a.executed,
        "recovery is bounded; it does not retry forever"
    );
    assert!(!out.healing.is_empty(), "healing was attempted and audited");
}

#[test]
fn ev_a4_suite_passes() {
    let sup = SelfHealing::new();
    for r in run_ev_a4(&sup) {
        assert!(r.passed(), "{} failed: {}", r.id, r.detail);
    }
}
