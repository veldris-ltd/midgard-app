//! AC-MI-2: given an OAG query, when answered, the response cites the
//! object-fabric evidence it used. This runs the full path against a **real**
//! fabric — `plan (A.2) → ground (governed, cited) → assemble (8 layers) → serve`
//! — so citations map to real objects and above-clearance evidence is excluded by
//! the fabric's own authorisation, not by the retriever. [FR-MI-2]

use std::collections::{BTreeMap, BTreeSet};

use agent_runtime::{respond, Pipeline, RespondConfig};
use fabric::{
    Cardinality, Fabric, FunctionRuntime, Governance, InMemoryStore, LinkInstance, LinkType,
    ObjectInstance, ObjectStore, ObjectType, PolicyContext, PropertyType, TypeRegistry, Value,
    ValueType,
};
use model_server::{reference_registry, NodeProfile, Server};
use policy::{
    Action, ClassificationLevel, Clearance, DefaultPdp, Effect, Marking, PolicySet, Purpose, Role,
    Rule, RuleId, Selector,
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

fn object_type(id: &str, props: &[(&str, ValueType)]) -> ObjectType {
    ObjectType {
        id: id.into(),
        api_name: {
            // "case" -> "Case"
            let mut c = id.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        },
        title: id.into(),
        properties: props
            .iter()
            .map(|(n, t)| PropertyType::new(n, *t, false))
            .collect(),
        interfaces: vec![],
    }
}

fn obj(id: &str, type_id: &str, props: &[(&str, &str)], level: u8) -> ObjectInstance {
    let mut properties = BTreeMap::new();
    for (k, v) in props {
        properties.insert((*k).to_string(), Value::Text((*v).to_string()));
    }
    ObjectInstance {
        id: id.into(),
        type_id: type_id.into(),
        properties,
        marking: Marking {
            level: ClassificationLevel(level),
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

fn pdp() -> DefaultPdp {
    DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("read".into()),
            role: Role("analyst".into()),
            action: Action("Read".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    )
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

fn build_fabric() -> Fabric<InMemoryStore> {
    let mut reg = TypeRegistry::new();
    reg.register_object_type(object_type(
        "case",
        &[("name", ValueType::Text), ("status", ValueType::Text)],
    ))
    .unwrap();
    reg.register_object_type(object_type("person", &[("name", ValueType::Text)]))
        .unwrap();
    reg.register_link_type(LinkType {
        id: "assigned_to".into(),
        api_name: "assignedTo".into(),
        source_type: "case".into(),
        target_type: "person".into(),
        cardinality: Cardinality::OneToMany,
    })
    .unwrap();

    let gov = Governance::new(pdp(), &mut SeedRng(9)).unwrap();
    let mut fab = Fabric::new(reg, InMemoryStore::new(), FunctionRuntime::new(), gov);
    fab.store_mut().put_object(obj(
        "case-1",
        "case",
        &[("name", "Alpha"), ("status", "open")],
        0,
    ));
    fab.store_mut()
        .put_object(obj("person-7", "person", &[("name", "Dana")], 0));
    // An above-clearance case the unclassified caller must never see cited.
    fab.store_mut().put_object(obj(
        "secret-9",
        "case",
        &[("name", "Hidden"), ("status", "open")],
        2,
    ));
    fab.store_mut().put_link(LinkInstance {
        link_type: "assigned_to".into(),
        source_id: "case-1".into(),
        target_id: "person-7".into(),
    });
    fab
}

fn server() -> Server {
    Server::bootstrap(NodeProfile::cpu_only(4), &reference_registry(), None).unwrap()
}

#[test]
fn ac_mi_2_answer_cites_fabric_evidence() {
    let mut fab = build_fabric();
    let srv = server();
    let pipeline = Pipeline::new();
    let cfg = RespondConfig {
        budget_tokens: 4096,
        tools: vec![],
        max_tokens: 16,
    };

    let resp = respond(
        "who is assigned to the case?",
        &ctx(),
        &mut fab,
        &srv,
        &pipeline,
        &cfg,
        1,
    )
    .expect("assembles and serves");

    assert!(resp.refusal.is_none(), "grounded task must be answered");
    let answer = resp.answer.as_deref().expect("an answer");
    assert!(!answer.is_empty());

    // The response cites the fabric evidence it used.
    assert!(!resp.citations.is_empty(), "must cite evidence");
    assert!(resp.citations.contains(&"obj:case-1".to_string()));
    // The assignee, reached via search-around, is cited.
    assert!(resp.citations.contains(&"obj:person-7".to_string()));
    // The above-clearance case is never retrieved, so never cited. [NFR-SEC-2]
    assert!(!resp.citations.contains(&"obj:secret-9".to_string()));
    // Every citation is a real object id.
    assert!(resp.citations.iter().all(|c| c.starts_with("obj:")));

    assert!(resp.plan.groundable);
    assert_eq!(resp.model_id, "local-reference");
}

#[test]
fn ungroundable_task_refuses_without_fabricating() {
    let mut fab = build_fabric();
    let srv = server();
    let resp = respond(
        "what will the weather be tomorrow?",
        &ctx(),
        &mut fab,
        &srv,
        &Pipeline::new(),
        &RespondConfig::default(),
        1,
    )
    .expect("responds");

    // No ontology type matches: refuse, do not guess, cite nothing.
    let refusal = resp.refusal.expect("must refuse");
    assert_eq!(refusal.code, "UNGROUNDED");
    assert!(resp.answer.is_none());
    assert!(resp.citations.is_empty());
    assert!(!resp.plan.groundable);
}
