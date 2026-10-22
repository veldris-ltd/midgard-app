//! What the console's object explorer relies on: a query finds objects by text and by
//! id and says how many matched; an action on one object changes only that object; a
//! cycle starts from its first value; and a human-gated action records, before it
//! runs, who approved it and why. [DD-1] [NFR-AI-1] [AC-M-4]
#![cfg(feature = "backend-fips")]

use std::collections::{BTreeMap, BTreeSet};

use fabric::{
    FabricError, InMemoryStore, LinkInstance, ObjectInstance, ObjectStore, PolicyContext, StoreOp,
    Value,
};
use governance::Governance;
use http_gateway::demo_operator;
use http_gateway::enclosure::{Enclosure, ACTION_HUMAN_GATE};
use http_gateway::json::Json;
use http_gateway::ontology_file::parse_ontology;
use policy::{
    Action, DefaultPdp, Effect, Marking, PolicySet, Purpose, Role, Rule, RuleId, Selector,
};
use pqc_agility::OsRng;

const NOW: u64 = 1_800_000_000;

const ONTOLOGY: &str = r#"{
  "version": 1,
  "objectTypes": [
    { "id": "airport", "title": "Airport",
      "properties": [ { "name": "name", "type": "text", "required": true },
                      { "name": "status", "type": "text" },
                      { "name": "closed", "type": "bool" } ] }
  ],
  "actions": [
    { "id": "toggle_airport", "name": "toggleAirport", "title": "Open or close airport",
      "objectType": "airport", "humanGated": true, "marking": "internal",
      "purposes": ["operations"],
      "effect": { "kind": "cycle-property", "property": "status", "values": ["open", "closed"] } },
    { "id": "shut_airport", "name": "shutAirport", "title": "Shut airport",
      "objectType": "airport", "humanGated": false, "marking": "internal",
      "purposes": ["operations"],
      "effect": { "kind": "cycle-property", "property": "closed", "values": ["false", "true"] } }
  ]
}"#;

fn rule(id: &str, action: &str) -> Rule {
    Rule {
        id: RuleId(id.into()),
        role: Role("operator".into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    }
}

fn enclosure() -> Enclosure {
    enclosure_over(InMemoryStore::new())
}

fn enclosure_over<S: ObjectStore>(store: S) -> Enclosure<S> {
    let policy = PolicySet::new(vec![
        rule("read", "Read"),
        rule("toggle", "toggleAirport"),
        rule("shut", "shutAirport"),
    ])
    .unwrap();
    let governance = Governance::new(DefaultPdp::new(policy), &mut OsRng).unwrap();
    let mut enclosure = Enclosure::with_store(store, governance);
    enclosure
        .load_ontology(&parse_ontology(ONTOLOGY).unwrap(), NOW)
        .unwrap();
    for (key, name) in [
        ("EGLL", "London Heathrow"),
        ("EGKK", "London Gatwick"),
        ("EGPH", "Edinburgh"),
    ] {
        enclosure
            .fabric_mut()
            .store_mut()
            .put_object(ObjectInstance {
                id: format!("airport/{key}"),
                type_id: "airport".into(),
                properties: [("name".to_string(), Value::Text(name.into()))].into(),
                marking: Marking {
                    level: http_gateway::enclosure::level_of("internal").unwrap(),
                    compartments: BTreeSet::new(),
                },
                allowed_purposes: [Purpose("operations".into())].into(),
            });
    }
    enclosure
}

fn alice() -> PolicyContext {
    let user = demo_operator();
    PolicyContext {
        subject: "alice".into(),
        roles: user.roles,
        clearance: user.clearance,
        purpose: Purpose("operations".into()),
    }
}

fn ids(result: &Json) -> Vec<String> {
    match result.get("results") {
        Some(Json::Arr(items)) => items
            .iter()
            .filter_map(|o| o.str_field("id").map(str::to_string))
            .collect(),
        _ => panic!("no results: {}", result.encode()),
    }
}

fn status(enclosure: &Enclosure, id: &str) -> Option<String> {
    match enclosure
        .fabric()
        .store()
        .get_object(id)?
        .properties
        .get("status")
    {
        Some(Value::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

#[test]
fn a_query_finds_by_text_and_by_id_and_counts_the_matches() {
    let mut enc = enclosure();
    let q = |enc: &mut Enclosure, max, search: Option<&str>, id: Option<&str>| {
        enc.query(
            &alice(),
            "airport",
            max,
            BTreeMap::new(),
            search.map(str::to_string),
            id.map(str::to_string),
            NOW,
        )
        .unwrap()
    };
    let london = q(&mut enc, 50, Some("LONDON"), None);
    assert_eq!(ids(&london), vec!["airport/EGKK", "airport/EGLL"]);
    assert_eq!(london.get("matched"), Some(&Json::Int(2)));

    let first = q(&mut enc, 1, None, None);
    assert_eq!(ids(&first).len(), 1);
    assert_eq!(
        first.get("matched"),
        Some(&Json::Int(3)),
        "counted before the cut"
    );

    assert_eq!(
        ids(&q(&mut enc, 5, Some("egph"), None)),
        vec!["airport/EGPH"],
        "keys too"
    );
    assert_eq!(
        ids(&q(&mut enc, 5, None, Some("airport/EGLL"))),
        vec!["airport/EGLL"]
    );
    assert!(
        ids(&q(&mut enc, 5, None, Some("airport/EG"))).is_empty(),
        "an id is exact"
    );
}

#[test]
fn a_gated_action_on_one_object_records_who_approved_it_and_why_first() {
    let mut enc = enclosure();
    let target: BTreeMap<String, String> =
        [("target".to_string(), "airport/EGLL".to_string())].into();

    let refused = enc.apply_action(&alice(), "Open or close airport", target.clone(), None, NOW);
    assert_eq!(
        refused.unwrap_err().status,
        403,
        "the gate needs an approval"
    );
    assert_eq!(status(&enc, "airport/EGLL"), None);

    let before = enc.fabric().audit_entries().len();
    let applied = enc
        .apply_action(
            &alice(),
            "Open or close airport",
            target.clone(),
            Some("runway inspection\n"),
            NOW,
        )
        .unwrap_or_else(|e| panic!("{}", e.body.encode()));
    assert!(
        applied.str_field("approvalAuditId").is_some(),
        "{}",
        applied.encode()
    );

    // A missing status counts as the first value: the first run closes it.
    assert_eq!(status(&enc, "airport/EGLL").as_deref(), Some("closed"));
    assert_eq!(
        status(&enc, "airport/EGKK"),
        None,
        "only the target changed"
    );

    let entries = enc.fabric().audit_entries();
    assert_eq!(entries.len(), before + 2, "the approval, then the action");
    let approval = &entries[before].record;
    assert_eq!(approval.actor.0, "alice");
    assert_eq!(approval.action.0, ACTION_HUMAN_GATE);
    assert_eq!(
        approval.resource.0,
        "action:toggle_airport target:airport/EGLL objects:1 reason:runway inspection"
    );
    assert_eq!(entries[before + 1].record.action.0, "PERMIT toggleAirport");

    enc.apply_action(
        &alice(),
        "Open or close airport",
        target,
        Some("reopened"),
        NOW,
    )
    .unwrap();
    assert_eq!(status(&enc, "airport/EGLL").as_deref(), Some("open"));
}

#[test]
fn a_true_false_property_is_cycled_as_a_boolean() {
    let mut enc = enclosure();
    let target: BTreeMap<String, String> =
        [("target".to_string(), "airport/EGKK".to_string())].into();
    let closed = |enc: &Enclosure| {
        let o = enc.fabric().store().get_object("airport/EGKK").unwrap();
        o.properties.get("closed").cloned()
    };
    enc.apply_action(&alice(), "Shut airport", target.clone(), None, NOW)
        .unwrap();
    assert_eq!(
        closed(&enc),
        Some(Value::Bool(true)),
        "missing counts as false"
    );
    enc.apply_action(&alice(), "Shut airport", target, None, NOW)
        .unwrap();
    assert_eq!(closed(&enc), Some(Value::Bool(false)));
}

/// An in-memory store whose batched writes fail once `broken` is set, as the durable
/// store's do when its disk does.
struct Breakable {
    inner: InMemoryStore,
    broken: bool,
}
impl ObjectStore for Breakable {
    fn put_object(&mut self, obj: ObjectInstance) {
        self.inner.put_object(obj)
    }
    fn get_object(&self, id: &str) -> Option<&ObjectInstance> {
        self.inner.get_object(id)
    }
    fn objects_of_type(&self, type_id: &str) -> Vec<&ObjectInstance> {
        self.inner.objects_of_type(type_id)
    }
    fn by_property(&self, type_id: &str, property: &str, key: &str) -> Vec<&ObjectInstance> {
        self.inner.by_property(type_id, property, key)
    }
    fn put_link(&mut self, link: LinkInstance) {
        self.inner.put_link(link)
    }
    fn neighbors(&self, source_id: &str, link_type: &str) -> Vec<&ObjectInstance> {
        self.inner.neighbors(source_id, link_type)
    }
    fn remove_object(&mut self, id: &str) -> Option<ObjectInstance> {
        self.inner.remove_object(id)
    }
    fn try_apply_all(&mut self, ops: Vec<StoreOp>) -> Result<(), FabricError> {
        if self.broken {
            return Err(FabricError::Store("the disk is gone".into()));
        }
        self.inner.try_apply_all(ops)
    }
}

#[test]
fn an_action_that_cannot_be_written_is_an_error_and_the_approval_stays_recorded() {
    let mut enc = enclosure_over(Breakable {
        inner: InMemoryStore::new(),
        broken: false,
    });
    enc.fabric_mut().store_mut().broken = true;
    let target: BTreeMap<String, String> =
        [("target".to_string(), "airport/EGLL".to_string())].into();
    let before = enc.fabric().audit_entries().len();
    let err = enc
        .apply_action(
            &alice(),
            "Open or close airport",
            target,
            Some("runway inspection"),
            NOW,
        )
        .unwrap_err();
    assert_eq!(err.status, 500, "{}", err.body.encode());
    assert!(
        err.body.encode().contains("the disk is gone"),
        "{}",
        err.body.encode()
    );
    let status = enc
        .fabric()
        .store()
        .get_object("airport/EGLL")
        .unwrap()
        .properties
        .get("status")
        .cloned();
    assert_eq!(status, None, "nothing applied");
    // Who approved it and the permit are recorded before the write, as always; the
    // error tells the caller it did not happen.
    let entries = enc.fabric().audit_entries();
    assert_eq!(entries.len(), before + 2);
    assert_eq!(entries[before].record.action.0, ACTION_HUMAN_GATE);
    assert_eq!(entries[before + 1].record.action.0, "PERMIT toggleAirport");
}
