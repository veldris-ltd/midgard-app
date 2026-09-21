//! Consolidated per-EV report (Prompt 2.f): runs the A.1–A.5 Eval suites and
//! prints PASS/FAIL per EV id with real output (`cargo test -- --nocapture`),
//! failing closed if any eval fails. The per-AC report is driven by
//! `deploy/acceptance/verify-mimir.sh`; this is its EV counterpart.

use std::collections::{BTreeMap, BTreeSet};

use agent_runtime::builder::gate_resource;
use agent_runtime::{
    ev_a1_ground, ev_a1_mutation, ev_a1_notool, ev_a1_policy, ev_a1_schema, ev_a1_ungrounded,
    run_ev_a2, run_ev_a3, run_ev_a4, run_ev_a5, ActionIntent, AgentOutput, AgentRefusal,
    EvalResult, FeedbackDistiller, LogicBuilder, ProposedAction, RefusalCode, SelfDefending,
    SelfHealing,
};
use fabric::{
    ActionFunction, ActionType, Effect, Fabric, FabricError, FunctionRuntime, Governance,
    InMemoryStore, ObjectInstance, ObjectStore, ObjectType, PolicyContext, PropertyType,
    TypeRegistry, Value, ValueType,
};
use oag_retriever::{Grounding, GroundingFact};
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

// ---- EV-A1 output-level fixtures -----------------------------------------

fn grounding_one() -> Grounding {
    Grounding {
        facts: vec![GroundingFact {
            citation: "obj:case-1".into(),
            object_type: "case".into(),
            properties: vec![],
            marking_level: 0,
            relevance: 100,
        }],
        audit_seqs: vec![],
    }
}

fn ev_a1_output_level() -> Vec<EvalResult> {
    let g = grounding_one();
    let grounded = AgentOutput {
        answer: Some("cited answer".into()),
        citations: vec!["obj:case-1".into()],
        ..Default::default()
    };
    let refused = AgentOutput {
        answer: None,
        refusal: Some(AgentRefusal {
            code: RefusalCode::Ungrounded,
            detail: String::new(),
        }),
        ..Default::default()
    };
    let scoped = AgentOutput {
        proposed_actions: vec![ProposedAction {
            action_type: "triage".into(),
            params: BTreeMap::new(),
            rationale: String::new(),
            citations: vec![],
        }],
        ..Default::default()
    };
    vec![
        ev_a1_schema(&grounded),
        ev_a1_ground(&grounded, &g),
        ev_a1_ungrounded(&refused, &Grounding::default()),
        ev_a1_notool(&scoped, &["triage".to_string()]),
    ]
}

// ---- EV-A1 loop-level: a real governed run -------------------------------

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

fn ev_a1_loop_level() -> Vec<EvalResult> {
    let gov = Governance::new(
        DefaultPdp::new(PolicySet::new(vec![]).unwrap()),
        &mut SeedRng(9),
    )
    .unwrap();
    let spec = LogicBuilder::new("triage-agent")
        .tool("triage", true)
        .preauthorise("triage", "investigation")
        .build();
    let l = LogicBuilder::new("triage-agent")
        .tool("triage", true)
        .preauthorise("triage", "investigation")
        .build_loop(gov, gate_resource());
    let ctx = PolicyContext {
        subject: "alice".into(),
        roles: [Role("analyst".into())].into_iter().collect(),
        clearance: Clearance {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        purpose: Purpose("investigation".into()),
    };
    let mut fab = build_fabric();
    let intent = Some(ActionIntent {
        action_type: "triage".into(),
        params: BTreeMap::from([("case_id".to_string(), "case-1".to_string())]),
        rationale: "needs triage".into(),
    });
    let out = l.run(&spec.task("triage the case", intent), &ctx, &mut fab, 100);
    vec![ev_a1_mutation(&out), ev_a1_policy(&out)]
}

#[test]
fn consolidated_ev_report() {
    let mut all: Vec<EvalResult> = Vec::new();
    all.extend(ev_a1_output_level());
    all.extend(ev_a1_loop_level());
    all.extend(run_ev_a2());
    all.extend(run_ev_a3(&SelfDefending::new()));
    all.extend(run_ev_a4(&SelfHealing::new()));
    all.extend(run_ev_a5(&FeedbackDistiller::new()));

    println!("\n=== Mimir prompt-level Evals (EV-A1..A5) ===");
    let mut passed = 0;
    for r in &all {
        let verdict = if r.passed() { "PASS" } else { "FAIL" };
        println!("  {:<20} {verdict}  {}", r.id, r.detail);
        if r.passed() {
            passed += 1;
        }
    }
    println!("  ------------------------------------------------------------");
    println!("  {passed}/{} evals passed", all.len());

    let failed: Vec<&str> = all
        .iter()
        .filter(|r| !r.passed())
        .map(|r| r.id.as_str())
        .collect();
    assert!(failed.is_empty(), "failing evals: {failed:?}");
    // 4 (A1 output) + 2 (A1 loop) + 3 (A2) + 4 (A3) + 4 (A4) + 4 (A5) = 21.
    assert_eq!(all.len(), 21);
}
