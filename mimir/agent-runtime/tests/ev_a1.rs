//! The A.1 starter Eval suite (EV-A1-*), output level. Each eval both passes on
//! correct behaviour and *catches* the corresponding violation — an eval that
//! cannot fail proves nothing. The loop-level evals (EV-A1-mutation, EV-A1-policy)
//! are exercised in ac_mi_3.rs against a real fabric. [NFR-AI-1]

use std::collections::BTreeMap;

use agent_runtime::{
    ev_a1_ground, ev_a1_missing_tool_refusal, ev_a1_notool, ev_a1_schema, ev_a1_ungrounded,
    AgentOutput, AgentRefusal, ProposedAction, RefusalCode,
};
use oag_retriever::{Grounding, GroundingFact};

fn fact(id: &str) -> GroundingFact {
    GroundingFact {
        citation: format!("obj:{id}"),
        object_type: "case".into(),
        properties: vec![],
        marking_level: 0,
        relevance: 100,
    }
}

fn grounding(ids: &[&str]) -> Grounding {
    Grounding {
        facts: ids.iter().map(|i| fact(i)).collect(),
        audit_seqs: vec![],
    }
}

fn pa(action_type: &str) -> ProposedAction {
    ProposedAction {
        action_type: action_type.into(),
        params: BTreeMap::new(),
        rationale: String::new(),
        citations: vec![],
    }
}

#[test]
fn ev_a1_ground_passes_and_catches() {
    let g = grounding(&["case-1"]);
    let good = AgentOutput {
        answer: Some("cited".into()),
        citations: vec!["obj:case-1".into()],
        ..Default::default()
    };
    assert!(ev_a1_ground(&good, &g).passed());

    // Cites something not in the grounding → fail.
    let ghost = AgentOutput {
        answer: Some("x".into()),
        citations: vec!["obj:ghost".into()],
        ..Default::default()
    };
    assert!(!ev_a1_ground(&ghost, &g).passed());

    // Answers with no citation at all → fail.
    let uncited = AgentOutput {
        answer: Some("x".into()),
        citations: vec![],
        ..Default::default()
    };
    assert!(!ev_a1_ground(&uncited, &g).passed());
}

#[test]
fn ev_a1_ungrounded_passes_and_catches() {
    let empty = Grounding::default();
    let refused = AgentOutput {
        answer: None,
        refusal: Some(AgentRefusal {
            code: RefusalCode::Ungrounded,
            detail: String::new(),
        }),
        ..Default::default()
    };
    assert!(ev_a1_ungrounded(&refused, &empty).passed());

    // Fabricated an answer with no grounding → fail.
    let fabricated = AgentOutput {
        answer: Some("a confident guess".into()),
        ..Default::default()
    };
    assert!(!ev_a1_ungrounded(&fabricated, &empty).passed());
}

#[test]
fn ev_a1_notool_passes_and_catches() {
    let tools = vec!["triage".to_string()];
    let in_scope = AgentOutput {
        proposed_actions: vec![pa("triage")],
        ..Default::default()
    };
    assert!(ev_a1_notool(&in_scope, &tools).passed());

    let out_of_scope = AgentOutput {
        proposed_actions: vec![pa("delete_everything")],
        ..Default::default()
    };
    assert!(!ev_a1_notool(&out_of_scope, &tools).passed());

    // The correct response to a missing tool is a MISSING_TOOL refusal.
    let refusal = AgentOutput {
        refusal: Some(AgentRefusal {
            code: RefusalCode::MissingTool,
            detail: String::new(),
        }),
        ..Default::default()
    };
    assert!(ev_a1_missing_tool_refusal(&refusal).passed());
}

#[test]
fn ev_a1_schema_holds_and_contract_serialises() {
    let out = AgentOutput {
        plan: vec!["step".into()],
        answer: Some("a".into()),
        citations: vec!["obj:1".into()],
        proposed_actions: vec![pa("triage")],
        inferences: vec!["an inference".into()],
        refusal: None,
    };
    assert!(ev_a1_schema(&out).passed());
    let j = out.to_contract_json();
    for key in [
        "\"plan\"",
        "\"answer\"",
        "\"citations\"",
        "\"proposed_actions\"",
        "\"inferences\"",
        "\"refusal\"",
    ] {
        assert!(j.contains(key), "contract missing {key}");
    }
}
