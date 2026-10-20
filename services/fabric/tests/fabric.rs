//! Object Fabric: semantic schema validation, incremental-index queries,
//! dynamic security at query time, search-around, and governed + audited actions.
//! [DD-1] [NFR-SEC-2] [NFR-AI-1]

use std::collections::{BTreeMap, BTreeSet};

use audit_writer::verify_chain;
use fabric::{
    ActionFunction, ActionType, Cardinality, Effect, Fabric, FabricError, FunctionRuntime,
    Governance, InMemoryStore, Interface, LinkInstance, LinkType, ObjectInstance, ObjectStore,
    ObjectType, PolicyContext, PropertyType, Query, TypeRegistry, Value, ValueType, FABRIC_SIG_ALG,
};
use policy::{
    Action, ClassificationLevel, Clearance, Compartment, DefaultPdp, Effect as PolicyEffect,
    Marking, PolicySet, Purpose, Role, Rule, RuleId, Selector,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, SigId, VerifyingKey};

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

// ---- schema helpers -------------------------------------------------------

fn named_interface() -> Interface {
    Interface {
        api_name: "Named".into(),
        properties: vec![PropertyType::new("name", ValueType::Text, true)],
    }
}

fn person_type() -> ObjectType {
    ObjectType {
        id: "person".into(),
        api_name: "Person".into(),
        title: "Person".into(),
        properties: vec![
            PropertyType::new("name", ValueType::Text, true),
            PropertyType::new("department", ValueType::Text, false),
        ],
        interfaces: vec!["Named".into()],
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

fn marking(level: ClassificationLevel) -> Marking {
    Marking {
        level,
        compartments: BTreeSet::new(),
    }
}

fn purposes(ps: &[&str]) -> BTreeSet<Purpose> {
    ps.iter().map(|p| Purpose(p.to_string())).collect()
}

fn person(
    id: &str,
    name: &str,
    dept: &str,
    level: ClassificationLevel,
    ps: &[&str],
) -> ObjectInstance {
    let mut properties = BTreeMap::new();
    properties.insert("name".into(), Value::Text(name.into()));
    properties.insert("department".into(), Value::Text(dept.into()));
    ObjectInstance {
        id: id.into(),
        type_id: "person".into(),
        properties,
        marking: marking(level),
        allowed_purposes: purposes(ps),
    }
}

fn case(id: &str, name: &str, status: &str) -> ObjectInstance {
    let mut properties = BTreeMap::new();
    properties.insert("name".into(), Value::Text(name.into()));
    properties.insert("status".into(), Value::Text(status.into()));
    ObjectInstance {
        id: id.into(),
        type_id: "case".into(),
        properties,
        marking: marking(ClassificationLevel::UNCLASSIFIED),
        allowed_purposes: purposes(&["investigation"]),
    }
}

fn rule(id: &str, role: &str, action: &str) -> Rule {
    Rule {
        id: RuleId(id.into()),
        role: Role(role.into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect: PolicyEffect::Permit,
    }
}

/// analyst may Read and close_case for the investigation purpose.
fn analyst_pdp() -> DefaultPdp {
    DefaultPdp::new(
        PolicySet::new(vec![
            rule("read", "analyst", "Read"),
            rule("close", "analyst", "close_case"),
        ])
        .unwrap(),
    )
}

fn ctx(subject: &str, roles: &[&str], level: ClassificationLevel, purpose: &str) -> PolicyContext {
    PolicyContext {
        subject: subject.into(),
        roles: roles.iter().map(|r| Role(r.to_string())).collect(),
        clearance: Clearance {
            level,
            compartments: BTreeSet::new(),
        },
        purpose: Purpose(purpose.into()),
    }
}

/// A close-case action function: sets the target case's status to "closed".
struct CloseCase;
impl ActionFunction for CloseCase {
    fn apply(
        &self,
        params: &BTreeMap<String, Value>,
        store: &dyn fabric::ObjectStore,
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
            .insert("status".into(), Value::Text("closed".into()));
        Ok(vec![Effect::PutObject(updated)])
    }
}

fn close_case_action() -> ActionType {
    ActionType {
        id: "close_case".into(),
        api_name: "close_case".into(),
        parameters: vec![PropertyType::new("case_id", ValueType::Text, true)],
        function_id: "fn_close_case".into(),
        marking: marking(ClassificationLevel::UNCLASSIFIED),
        allowed_purposes: purposes(&["investigation"]),
    }
}

/// Build a populated fabric: schema, a link, a function, and a policy.
fn build_fabric() -> Fabric<InMemoryStore> {
    let mut registry = TypeRegistry::new();
    registry.register_interface(named_interface()).unwrap();
    registry.register_object_type(person_type()).unwrap();
    registry.register_object_type(case_type()).unwrap();
    registry
        .register_link_type(LinkType {
            id: "assigned_to".into(),
            api_name: "assignedTo".into(),
            source_type: "case".into(),
            target_type: "person".into(),
            cardinality: Cardinality::OneToMany,
        })
        .unwrap();
    registry.register_action_type(close_case_action()).unwrap();

    let mut runtime = FunctionRuntime::new();
    runtime.register("fn_close_case", Box::new(CloseCase));

    let governance = Governance::new(analyst_pdp(), &mut SeedRng(1)).unwrap();
    Fabric::new(registry, InMemoryStore::new(), runtime, governance)
}

// ---- semantic schema validation -------------------------------------------

#[test]
fn schema_validation_fails_closed() {
    let mut r = TypeRegistry::new();
    r.register_interface(named_interface()).unwrap();
    r.register_object_type(person_type()).unwrap();

    // Duplicate type id.
    assert!(matches!(
        r.register_object_type(person_type()),
        Err(FabricError::DuplicateType(_))
    ));

    // Claims the Named interface but omits `name`.
    let bad = ObjectType {
        id: "widget".into(),
        api_name: "Widget".into(),
        title: "Widget".into(),
        properties: vec![PropertyType::new("sku", ValueType::Text, true)],
        interfaces: vec!["Named".into()],
    };
    assert!(matches!(
        r.register_object_type(bad),
        Err(FabricError::MissingInterfaceProperty { .. })
    ));

    // Link to a non-existent target type.
    assert!(matches!(
        r.register_link_type(LinkType {
            id: "l".into(),
            api_name: "l".into(),
            source_type: "person".into(),
            target_type: "ghost".into(),
            cardinality: Cardinality::OneToOne,
        }),
        Err(FabricError::InvalidLinkEndpoints(_))
    ));

    // Duplicate property name.
    let dup = ObjectType {
        id: "dupe".into(),
        api_name: "Dupe".into(),
        title: "Dupe".into(),
        properties: vec![
            PropertyType::new("x", ValueType::Text, true),
            PropertyType::new("x", ValueType::Int, false),
        ],
        interfaces: vec![],
    };
    assert!(matches!(
        r.register_object_type(dup),
        Err(FabricError::DuplicateProperty(_))
    ));
}

// ---- incremental index + typed query --------------------------------------

#[test]
fn indexed_query_by_type_and_property() {
    let mut fab = build_fabric();
    fab.store_mut().put_object(person(
        "p1",
        "Alice",
        "eng",
        ClassificationLevel::UNCLASSIFIED,
        &["investigation"],
    ));
    fab.store_mut().put_object(person(
        "p2",
        "Bob",
        "eng",
        ClassificationLevel::UNCLASSIFIED,
        &["investigation"],
    ));
    fab.store_mut().put_object(person(
        "p3",
        "Carol",
        "legal",
        ClassificationLevel::UNCLASSIFIED,
        &["investigation"],
    ));

    let c = ctx(
        "alice",
        &["analyst"],
        ClassificationLevel::SECRET,
        "investigation",
    );

    // All persons.
    let all = fab
        .query(
            &c,
            &Query {
                object_type: "person".into(),
                filter: BTreeMap::new(),
                link_traversals: vec![],
                max_results: 10,
                search: None,
            },
            100,
        )
        .unwrap();
    assert_eq!(all.results.len(), 3);

    // Inverted-index filter on department.
    let mut filter = BTreeMap::new();
    filter.insert("department".into(), "eng".into());
    let eng = fab
        .query(
            &c,
            &Query {
                object_type: "person".into(),
                filter,
                link_traversals: vec![],
                max_results: 10,
                search: None,
            },
            101,
        )
        .unwrap();
    assert_eq!(eng.results.len(), 2);

    // Update p3 into eng — the index must reflect it (incremental reindex).
    fab.store_mut().put_object(person(
        "p3",
        "Carol",
        "eng",
        ClassificationLevel::UNCLASSIFIED,
        &["investigation"],
    ));
    let mut filter = BTreeMap::new();
    filter.insert("department".into(), "eng".into());
    let eng2 = fab
        .query(
            &c,
            &Query {
                object_type: "person".into(),
                filter,
                link_traversals: vec![],
                max_results: 10,
                search: None,
            },
            102,
        )
        .unwrap();
    assert_eq!(eng2.results.len(), 3);
}

#[test]
fn unknown_object_type_is_rejected() {
    let mut fab = build_fabric();
    let c = ctx(
        "alice",
        &["analyst"],
        ClassificationLevel::SECRET,
        "investigation",
    );
    let err = fab
        .query(
            &c,
            &Query {
                object_type: "ghost".into(),
                filter: BTreeMap::new(),
                link_traversals: vec![],
                max_results: 10,
                search: None,
            },
            1,
        )
        .unwrap_err();
    assert!(matches!(err, FabricError::UnknownObjectType(_)));
}

// ---- dynamic security at query time ---------------------------------------

#[test]
fn query_enforces_marking_purpose_and_role() {
    let mut fab = build_fabric();
    // Readable.
    fab.store_mut().put_object(person(
        "ok",
        "Ok",
        "eng",
        ClassificationLevel::UNCLASSIFIED,
        &["investigation"],
    ));
    // Above the caller's clearance (no read-up).
    fab.store_mut().put_object(person(
        "ts",
        "Secret",
        "eng",
        ClassificationLevel::TOP_SECRET,
        &["investigation"],
    ));
    // Wrong purpose.
    fab.store_mut().put_object(person(
        "wp",
        "WrongPurpose",
        "eng",
        ClassificationLevel::UNCLASSIFIED,
        &["audit"],
    ));

    // SECRET analyst for investigation: sees only "ok".
    let c = ctx(
        "alice",
        &["analyst"],
        ClassificationLevel::SECRET,
        "investigation",
    );
    let res = fab
        .query(
            &c,
            &Query {
                object_type: "person".into(),
                filter: BTreeMap::new(),
                link_traversals: vec![],
                max_results: 10,
                search: None,
            },
            1,
        )
        .unwrap();
    let ids: Vec<&str> = res.results.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, vec!["ok"]);

    // A caller without the analyst role has no Read permit: the query itself is
    // denied at the governance point (and the denial is audited).
    let c2 = ctx(
        "mallory",
        &["guest"],
        ClassificationLevel::SECRET,
        "investigation",
    );
    let err = fab
        .query(
            &c2,
            &Query {
                object_type: "person".into(),
                filter: BTreeMap::new(),
                link_traversals: vec![],
                max_results: 10,
                search: None,
            },
            2,
        )
        .unwrap_err();
    assert!(matches!(err, FabricError::Denied));
}

// ---- search-around --------------------------------------------------------

#[test]
fn search_around_traverses_links_under_policy() {
    let mut fab = build_fabric();
    fab.store_mut().put_object(case("c1", "Case One", "open"));
    fab.store_mut().put_object(person(
        "p1",
        "Alice",
        "eng",
        ClassificationLevel::UNCLASSIFIED,
        &["investigation"],
    ));
    fab.store_mut().put_object(person(
        "p2",
        "TopSecret",
        "eng",
        ClassificationLevel::TOP_SECRET,
        &["investigation"],
    ));
    fab.store_mut().put_link(LinkInstance {
        link_type: "assigned_to".into(),
        source_id: "c1".into(),
        target_id: "p1".into(),
    });
    fab.store_mut().put_link(LinkInstance {
        link_type: "assigned_to".into(),
        source_id: "c1".into(),
        target_id: "p2".into(),
    });

    let c = ctx(
        "alice",
        &["analyst"],
        ClassificationLevel::SECRET,
        "investigation",
    );
    let res = fab
        .query(
            &c,
            &Query {
                object_type: "case".into(),
                filter: BTreeMap::new(),
                link_traversals: vec!["assigned_to".into()],
                max_results: 10,
                search: None,
            },
            1,
        )
        .unwrap();
    let ids: BTreeSet<&str> = res.results.iter().map(|o| o.id.as_str()).collect();
    // The case and the readable assignee — but NOT the TOP_SECRET assignee.
    assert!(ids.contains("c1"));
    assert!(ids.contains("p1"));
    assert!(
        !ids.contains("p2"),
        "search-around must not leak above clearance"
    );
}

// ---- governed, audited actions --------------------------------------------

#[test]
fn action_mutates_and_is_audited() {
    let mut fab = build_fabric();
    fab.store_mut().put_object(case("c1", "Case One", "open"));

    let c = ctx(
        "alice",
        &["analyst"],
        ClassificationLevel::SECRET,
        "investigation",
    );
    let mut params = BTreeMap::new();
    params.insert("case_id".into(), "c1".into());
    let resp = fab.apply_action(&c, "close_case", &params, 1000).unwrap();
    assert!(resp.applied);

    // The mutation is visible via query.
    let mut filter = BTreeMap::new();
    filter.insert("status".into(), "closed".into());
    let closed = fab
        .query(
            &c,
            &Query {
                object_type: "case".into(),
                filter,
                link_traversals: vec![],
                max_results: 10,
                search: None,
            },
            1001,
        )
        .unwrap();
    assert_eq!(closed.results.len(), 1);
    assert_eq!(closed.results[0].id, "c1");

    // The audit chain grew and verifies under the fabric's key. (2 entries:
    // the action + the query above.)
    assert!(fab.audit_entries().len() >= 2);
    let provider = fips_provider();
    let vk = fab.audit_verifying_key().clone();
    let ledger = fab.audit_ledger_id().clone();
    let resolver =
        move |alg: SigId| -> Option<VerifyingKey> { (alg == FABRIC_SIG_ALG).then(|| vk.clone()) };
    verify_chain(fab.audit_entries(), &provider, &ledger, &resolver).expect("audit chain verifies");
}

#[test]
fn action_denied_without_authorization() {
    let mut fab = build_fabric();
    fab.store_mut().put_object(case("c1", "Case One", "open"));
    let mut params = BTreeMap::new();
    params.insert("case_id".into(), "c1".into());

    // Missing the analyst role → denied (deny-by-default + required_role).
    let guest = ctx(
        "mallory",
        &["guest"],
        ClassificationLevel::SECRET,
        "investigation",
    );
    assert_eq!(
        fab.apply_action(&guest, "close_case", &params, 1)
            .unwrap_err(),
        FabricError::Denied
    );
    // The case is untouched.
    assert_eq!(
        fab.store()
            .get_object("c1")
            .unwrap()
            .properties
            .get("status"),
        Some(&Value::Text("open".into()))
    );
}

#[test]
fn action_rejects_bad_and_missing_params() {
    let mut fab = build_fabric();
    let c = ctx(
        "alice",
        &["analyst"],
        ClassificationLevel::SECRET,
        "investigation",
    );

    // Missing required param.
    assert_eq!(
        fab.apply_action(&c, "close_case", &BTreeMap::new(), 1)
            .unwrap_err(),
        FabricError::MissingRequiredParam("case_id".into())
    );

    // Unknown action type.
    assert!(matches!(
        fab.apply_action(&c, "nope", &BTreeMap::new(), 1),
        Err(FabricError::UnknownActionType(_))
    ));
}

#[test]
fn compartments_are_enforced_on_read() {
    // A subject cleared at SECRET but without the CREST compartment cannot read a
    // CREST-marked object (mandatory need-to-know).
    let mut fab = build_fabric();
    let mut compartments = BTreeSet::new();
    compartments.insert(Compartment("CREST".into()));
    let obj = ObjectInstance {
        id: "x".into(),
        type_id: "person".into(),
        properties: {
            let mut m = BTreeMap::new();
            m.insert("name".into(), Value::Text("Spy".into()));
            m
        },
        marking: Marking {
            level: ClassificationLevel::SECRET,
            compartments,
        },
        allowed_purposes: purposes(&["investigation"]),
    };
    fab.store_mut().put_object(obj);

    let c = ctx(
        "alice",
        &["analyst"],
        ClassificationLevel::SECRET,
        "investigation",
    );
    let res = fab
        .query(
            &c,
            &Query {
                object_type: "person".into(),
                filter: BTreeMap::new(),
                link_traversals: vec![],
                max_results: 10,
                search: None,
            },
            1,
        )
        .unwrap();
    assert!(
        res.results.is_empty(),
        "missing compartment must exclude the object"
    );
}
