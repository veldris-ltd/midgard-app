//! AC-MI-3: a state-changing agent action is Evals-checked and human-gated unless
//! pre-authorised for an evaluated scope. This drives the governed loop against a
//! real fabric (which authorises + audits the execution), covering every branch:
//! human-denied, human-approved, pre-authorised (no human), irreversible (forced
//! human), and policy-denied at execution. [NFR-AI-1]

use std::collections::BTreeMap;

use agent_runtime::{
    default_gate_resource, ev_a1_mutation, ev_a1_policy, ActionCatalog, ActionIntent, AgentTask,
    ApproveAll, DenyAll, GovernedLoop, HumanDecision, HumanGate, Preauth, StepBudget,
};
use fabric::{
    ActionFunction, ActionType, Effect, Fabric, FabricError, FunctionRuntime, Governance,
    InMemoryStore, ObjectInstance, ObjectStore, ObjectType, PolicyContext, PropertyType,
    TypeRegistry, Value, ValueType,
};
use policy::{
    Action, ClassificationLevel, Clearance, DefaultPdp, Effect as PEffect, Marking, PolicySet,
    Purpose, Role, Rule, RuleId, Selector,
};
use pqc_agility::Rng;
use std::collections::BTreeSet;

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

/// A reversible triage action: set case status to "triaged".
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

fn case_type() -> ObjectType {
    ObjectType {
        id: "case".into(),
        api_name: "Case".into(),
        title: "Case".into(),
        properties: vec![
            PropertyType::new("name", ValueType::Text, true),
            PropertyType::new("status", ValueType::Text, false),
        ],
        interfaces: vec![],
    }
}

fn triage_action() -> ActionType {
    ActionType {
        id: "triage".into(),
        api_name: "triage".into(),
        parameters: vec![PropertyType::new("case_id", ValueType::Text, true)],
        function_id: "fn_triage".into(),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

fn case(id: &str) -> ObjectInstance {
    let mut properties = BTreeMap::new();
    properties.insert("name".into(), Value::Text("Case".into()));
    properties.insert("status".into(), Value::Text("open".into()));
    ObjectInstance {
        id: id.into(),
        type_id: "case".into(),
        properties,
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

fn rule(id: &str, action: &str) -> Rule {
    Rule {
        id: RuleId(id.into()),
        role: Role("analyst".into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect: PEffect::Permit,
    }
}

/// A fabric with case-1; `permit_triage` controls whether the analyst may run the
/// triage action (used to exercise policy denial at execution).
fn build_fabric(permit_triage: bool) -> Fabric<InMemoryStore> {
    let mut reg = TypeRegistry::new();
    reg.register_object_type(case_type()).unwrap();
    reg.register_action_type(triage_action()).unwrap();
    let mut runtime = FunctionRuntime::new();
    runtime.register("fn_triage", Box::new(TriageCase));

    let mut rules = vec![rule("read", "Read")];
    if permit_triage {
        rules.push(rule("triage", "triage"));
    }
    let gov = Governance::new(
        DefaultPdp::new(PolicySet::new(rules).unwrap()),
        &mut SeedRng(5),
    )
    .unwrap();
    let mut fab = Fabric::new(reg, InMemoryStore::new(), runtime, gov);
    fab.store_mut().put_object(case("case-1"));
    fab
}

fn build_loop(human: Box<dyn HumanGate>, catalog: ActionCatalog, preauth: Preauth) -> GovernedLoop {
    let gov = Governance::new(
        DefaultPdp::new(PolicySet::new(vec![]).unwrap()),
        &mut SeedRng(9),
    )
    .unwrap();
    GovernedLoop::new(gov, default_gate_resource())
        .with_human(human)
        .with_catalog(catalog)
        .with_preauth(preauth)
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
            rationale: "the case needs triage".into(),
        }),
        budget: StepBudget::default(),
    }
}

fn status_of(fab: &Fabric<InMemoryStore>) -> String {
    match fab
        .store()
        .get_object("case-1")
        .and_then(|o| o.properties.get("status"))
    {
        Some(Value::Text(s)) => s.clone(),
        _ => "<none>".into(),
    }
}

#[test]
fn mutation_is_denied_when_human_denies() {
    let mut fab = build_fabric(true);
    let l = build_loop(
        Box::new(DenyAll),
        ActionCatalog::new().with("triage", true),
        Preauth::new(),
    );
    let out = l.run(&task(), &ctx(), &mut fab, 100);

    assert!(out.halted.is_none());
    let a = &out.actions[0];
    assert!(a.evaluated, "action was evals-checked");
    assert!(
        a.human_consulted,
        "not pre-authorised → human gate consulted"
    );
    assert_eq!(a.human_decision, Some(HumanDecision::Deny));
    assert!(!a.executed, "human denied → no mutation");
    assert_eq!(status_of(&fab), "open", "state unchanged");
    assert!(l.audit_len() >= 1, "gate decision is audited (INV-6)");
}

#[test]
fn mutation_executes_when_human_approves() {
    let mut fab = build_fabric(true);
    let l = build_loop(
        Box::new(ApproveAll),
        ActionCatalog::new().with("triage", true),
        Preauth::new(),
    );
    let out = l.run(&task(), &ctx(), &mut fab, 200);

    let a = &out.actions[0];
    assert!(a.evaluated && a.human_consulted);
    assert_eq!(a.human_decision, Some(HumanDecision::Approve));
    assert!(a.executed, "human approved → mutation applied");
    assert_eq!(status_of(&fab), "triaged");
}

#[test]
fn preauthorised_evaluated_scope_executes_without_human() {
    // The heart of AC-MI-3: a reversible action pre-authorised for this purpose
    // executes with NO human gate — even under a deny-all human — because it is a
    // pre-authorised, evaluated scope. It is still evals-checked.
    let mut fab = build_fabric(true);
    let l = build_loop(
        Box::new(DenyAll),
        ActionCatalog::new().with("triage", true),
        Preauth::new().allow("triage", "investigation"),
    );
    let out = l.run(&task(), &ctx(), &mut fab, 300);

    let a = &out.actions[0];
    assert!(a.evaluated, "still evals-checked");
    assert!(
        a.preauthorised,
        "ran under a pre-authorised evaluated scope"
    );
    assert!(
        !a.human_consulted,
        "no human gate for a pre-authorised scope"
    );
    assert_eq!(a.human_decision, None);
    assert!(a.executed);
    assert_eq!(status_of(&fab), "triaged");
}

#[test]
fn irreversible_action_forces_human_even_if_preauthorised() {
    // INV-5: an irreversible action can never be pre-authorised away from a human.
    let mut fab = build_fabric(true);
    let l = build_loop(
        Box::new(DenyAll),
        ActionCatalog::new().with("triage", false), // irreversible
        Preauth::new().allow("triage", "investigation"),
    );
    let out = l.run(&task(), &ctx(), &mut fab, 400);

    let a = &out.actions[0];
    assert!(!a.reversible);
    assert!(!a.preauthorised, "irreversible ⇒ not pre-authorisable");
    assert!(a.human_consulted, "human sign-off forced");
    assert!(
        !a.executed,
        "deny-all human blocked the irreversible mutation"
    );
    assert_eq!(status_of(&fab), "open");
}

#[test]
fn policy_denial_at_execution_beats_human_approval() {
    // Human approves, but the fabric's policy denies the action → not executed.
    let mut fab = build_fabric(false); // analyst not permitted to triage
    let l = build_loop(
        Box::new(ApproveAll),
        ActionCatalog::new().with("triage", true),
        Preauth::new(),
    );
    let out = l.run(&task(), &ctx(), &mut fab, 500);

    let a = &out.actions[0];
    assert_eq!(a.human_decision, Some(HumanDecision::Approve));
    assert!(
        !a.executed,
        "policy denied execution despite human approval"
    );
    assert!(a.policy_denied);
    assert_eq!(status_of(&fab), "open");

    // The loop-level EV-A1-* invariants hold for this run.
    assert!(ev_a1_policy(&out).passed());
    assert!(ev_a1_mutation(&out).passed());
}

#[test]
fn ev_a1_mutation_holds_for_a_gated_execution() {
    let mut fab = build_fabric(true);
    let l = build_loop(
        Box::new(ApproveAll),
        ActionCatalog::new().with("triage", true),
        Preauth::new(),
    );
    let out = l.run(&task(), &ctx(), &mut fab, 600);
    assert!(out.executed_any());
    assert!(ev_a1_mutation(&out).passed());
}
