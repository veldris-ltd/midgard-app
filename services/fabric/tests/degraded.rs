//! AC-M-9: a product built on core operates in a defined degraded mode when Mimir
//! (inference) is absent — and transparently uses it when present. The core has no
//! hard dependency on Mimir. [DD-6]

use std::collections::{BTreeMap, BTreeSet};

use fabric::{
    ActionFunction, ActionType, Effect, Fabric, FabricError, FunctionRuntime, Governance,
    InMemoryStore, Inference, ObjectInstance, ObjectStore, ObjectType, PolicyContext, PropertyType,
    Query, TypeRegistry, Value, ValueType,
};
use policy::{
    Action, ClassificationLevel, Clearance, DefaultPdp, Effect as PolicyEffect, Marking, PolicySet,
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

/// A stub inference plane (stands in for Mimir) that always suggests "ai-triaged".
struct StubMimir;
impl Inference for StubMimir {
    fn suggest(&self, _key: &str) -> Option<String> {
        Some("ai-triaged".to_string())
    }
}

/// An action function that *prefers* an AI-suggested status but falls back to a
/// deterministic rule when inference is absent (degraded mode). The optional
/// capability is held by the function itself — core requires nothing.
struct TriageCase {
    inference: Option<Box<dyn Inference>>,
}
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
        // Prefer Mimir's suggestion; degrade to a deterministic rule if absent.
        let status = match &self.inference {
            Some(mimir) => mimir.suggest(&case_id).unwrap_or_else(|| "triaged".into()),
            None => "triaged".into(),
        };
        let mut updated = existing.clone();
        updated
            .properties
            .insert("status".into(), Value::Text(status));
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

fn pdp() -> DefaultPdp {
    DefaultPdp::new(
        PolicySet::new(vec![
            Rule {
                id: RuleId("read".into()),
                role: Role("analyst".into()),
                action: Action("Read".into()),
                purpose: None,
                selector: Selector::Any,
                effect: PolicyEffect::Permit,
            },
            Rule {
                id: RuleId("triage".into()),
                role: Role("analyst".into()),
                action: Action("triage".into()),
                purpose: None,
                selector: Selector::Any,
                effect: PolicyEffect::Permit,
            },
        ])
        .unwrap(),
    )
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

fn ctx() -> PolicyContext {
    PolicyContext {
        subject: "alice".into(),
        roles: [Role("analyst".into())].into_iter().collect(),
        clearance: Clearance {
            level: ClassificationLevel::SECRET,
            compartments: BTreeSet::new(),
        },
        purpose: Purpose("investigation".into()),
    }
}

fn build(inference: Option<Box<dyn Inference>>, seed: u64) -> Fabric<InMemoryStore> {
    let mut registry = TypeRegistry::new();
    registry.register_object_type(case_type()).unwrap();
    registry.register_action_type(triage_action()).unwrap();
    let mut runtime = FunctionRuntime::new();
    runtime.register("fn_triage", Box::new(TriageCase { inference }));
    let governance = Governance::new(pdp(), &mut SeedRng(seed)).unwrap();
    let mut fab = Fabric::new(registry, InMemoryStore::new(), runtime, governance);
    fab.store_mut().put_object(case("c1"));
    fab
}

fn status_of(fab: &mut Fabric<InMemoryStore>) -> String {
    let mut filter = BTreeMap::new();
    filter.insert("name".into(), "Case".into());
    let res = fab
        .query(
            &ctx(),
            &Query {
                object_type: "case".into(),
                filter,
                link_traversals: vec![],
                max_results: 10,
                search: None,
            },
            1,
        )
        .unwrap();
    match res.results[0].properties.get("status") {
        Some(Value::Text(s)) => s.clone(),
        _ => "<none>".into(),
    }
}

#[test]
fn operates_in_degraded_mode_without_mimir() {
    // Mimir absent: the action still runs, using the deterministic fallback.
    let mut fab = build(None, 1);
    let mut params = BTreeMap::new();
    params.insert("case_id".into(), "c1".into());
    let resp = fab.apply_action(&ctx(), "triage", &params, 100).unwrap();
    assert!(resp.applied, "core action must succeed with Mimir absent");
    assert_eq!(status_of(&mut fab), "triaged");
}

#[test]
fn uses_mimir_when_present() {
    // Mimir present: the same action transparently uses the AI suggestion.
    let mut fab = build(Some(Box::new(StubMimir)), 2);
    let mut params = BTreeMap::new();
    params.insert("case_id".into(), "c1".into());
    fab.apply_action(&ctx(), "triage", &params, 100).unwrap();
    assert_eq!(status_of(&mut fab), "ai-triaged");
}
