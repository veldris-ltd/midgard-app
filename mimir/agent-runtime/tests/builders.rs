//! FR-MI-4: the no-code Logic builder and the pro-code builder produce the SAME
//! agent contract and an equivalent governed loop. A Logic-authored agent and a
//! pro-code-authored one behave identically for the same input; the pro-code
//! builder additionally lets an author swap in custom components.

use std::collections::{BTreeMap, BTreeSet};

use agent_runtime::builder::gate_resource;
use agent_runtime::{ActionIntent, AgentSpec, DenyAll, LogicBuilder, ProCodeBuilder};
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

fn gov() -> Governance {
    Governance::new(
        DefaultPdp::new(PolicySet::new(vec![]).unwrap()),
        &mut SeedRng(9),
    )
    .unwrap()
}

fn intent() -> Option<ActionIntent> {
    Some(ActionIntent {
        action_type: "triage".into(),
        params: BTreeMap::from([("case_id".to_string(), "case-1".to_string())]),
        rationale: "needs triage".into(),
    })
}

/// A no-code Logic agent: declare a reversible tool, pre-authorise it. Its
/// pre-authorised, reversible action executes without a human gate.
#[test]
fn logic_builder_agent_runs_over_the_contract() {
    let spec: AgentSpec = LogicBuilder::new("triage-agent")
        .tool("triage", true)
        .preauthorise("triage", "investigation")
        .build();
    assert_eq!(spec.tool_names(), vec!["triage".to_string()]);

    let l = LogicBuilder::new("triage-agent")
        .tool("triage", true)
        .preauthorise("triage", "investigation")
        .build_loop(gov(), gate_resource());

    let mut fab = build_fabric();
    let out = l.run(
        &spec.task("triage the case", intent()),
        &ctx(),
        &mut fab,
        100,
    );
    let a = &out.actions[0];
    assert!(a.preauthorised && !a.human_consulted && a.executed);
}

/// The pro-code builder, from the SAME spec, yields the same behaviour.
#[test]
fn pro_code_builder_from_same_spec_is_equivalent() {
    let spec = LogicBuilder::new("triage-agent")
        .tool("triage", true)
        .preauthorise("triage", "investigation")
        .build();

    let l = ProCodeBuilder::from_spec(spec.clone()).build_loop(gov(), gate_resource());
    let mut fab = build_fabric();
    let out = l.run(
        &spec.task("triage the case", intent()),
        &ctx(),
        &mut fab,
        200,
    );
    let a = &out.actions[0];
    assert!(a.preauthorised && !a.human_consulted && a.executed);
}

/// The pro-code builder can swap in a stricter human gate; the same spec now
/// requires (and here is denied) human authorisation because it is not
/// pre-authorised.
#[test]
fn pro_code_override_changes_behaviour() {
    // No pre-authorisation this time → the human gate governs.
    let spec = LogicBuilder::new("triage-agent")
        .tool("triage", true)
        .build();
    let l = ProCodeBuilder::from_spec(spec.clone())
        .with_human(Box::new(DenyAll))
        .build_loop(gov(), gate_resource());

    let mut fab = build_fabric();
    let out = l.run(
        &spec.task("triage the case", intent()),
        &ctx(),
        &mut fab,
        300,
    );
    let a = &out.actions[0];
    assert!(
        a.human_consulted && !a.executed,
        "deny-all human blocks the mutation"
    );
}

/// A no-code AI-function with no tools is a pure grounded Q&A agent.
#[test]
fn no_tool_agent_is_pure_qa() {
    let spec = LogicBuilder::new("qa-agent").build();
    let l = spec_loop(&spec);
    let mut fab = build_fabric();
    let out = l.run(
        &spec.task("who is assigned to the case?", None),
        &ctx(),
        &mut fab,
        400,
    );
    assert!(out.actions.is_empty());
    assert!(out.output.answer.is_some());
    assert!(!out.output.citations.is_empty());
}

fn spec_loop(spec: &AgentSpec) -> agent_runtime::GovernedLoop {
    ProCodeBuilder::from_spec(spec.clone()).build_loop(gov(), gate_resource())
}
