//! Retriever mechanics (FR-MI-2): the A.2 planner references only existing
//! ontology types, refuses ungroundable tasks, and grounding binds an `obj:<id>`
//! citation to every retrieved object with deterministic relevance ranking. The
//! "never above clearance" invariant and the full AC-MI-2 end-to-end are proven
//! against a real fabric in the agent-runtime tests.

use std::collections::{BTreeMap, BTreeSet};

use fabric::{ObjectInstance, Query, Value};
use oag_retriever::{
    formulate_plan, ground, GroundingSource, LinkCard, OntologySummary, PolicyContext, Retrieved,
    TypeCard,
};
use policy::{ClassificationLevel, Clearance, Marking, Purpose, Role};

/// A minimal grounding source: a fixed ontology and objects keyed by type id.
struct MockSource {
    summary: OntologySummary,
    objects: BTreeMap<String, Vec<ObjectInstance>>,
}

impl GroundingSource for MockSource {
    fn ontology_summary(&self) -> OntologySummary {
        self.summary.clone()
    }
    fn retrieve(
        &mut self,
        _ctx: &PolicyContext,
        q: &Query,
        _now: u64,
    ) -> Result<Retrieved, String> {
        Ok(Retrieved {
            objects: self
                .objects
                .get(&q.object_type)
                .cloned()
                .unwrap_or_default(),
            audit_seq: 7,
        })
    }
}

fn summary() -> OntologySummary {
    OntologySummary {
        objects: vec![
            TypeCard {
                id: "case".into(),
                api_name: "Case".into(),
                properties: vec!["name".into(), "status".into()],
            },
            TypeCard {
                id: "person".into(),
                api_name: "Person".into(),
                properties: vec!["name".into()],
            },
        ],
        links: vec![LinkCard {
            id: "assigned_to".into(),
            api_name: "assignedTo".into(),
            source: "case".into(),
            target: "person".into(),
        }],
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

fn unclassified() -> Clearance {
    Clearance {
        level: ClassificationLevel::UNCLASSIFIED,
        compartments: BTreeSet::new(),
    }
}

#[test]
fn plan_references_only_existing_types_and_adds_search_around() {
    let s = summary();
    let plan = formulate_plan("summarise the case", &s, &unclassified());
    assert!(plan.groundable);
    assert_eq!(plan.retrievals.len(), 1);
    assert_eq!(plan.retrievals[0].object_type, "case");
    // Outbound search-around to the assignee, even though "person" was not named.
    assert_eq!(
        plan.retrievals[0].link_traversals,
        vec!["assigned_to".to_string()]
    );
    // Every referenced type exists in the summary (A.2 rule 2).
    assert!(plan.retrievals.iter().all(|r| s.has_object(&r.object_type)));
}

#[test]
fn ungroundable_task_returns_false_with_reason() {
    let plan = formulate_plan("what is the weather tomorrow", &summary(), &unclassified());
    assert!(!plan.groundable);
    assert!(plan.retrievals.is_empty());
    assert!(plan.reason_if_not.is_some());
}

#[test]
fn grounding_binds_citations_and_ranks_by_relevance() {
    let mut src = MockSource {
        summary: summary(),
        objects: BTreeMap::from([(
            "case".to_string(),
            vec![
                obj("case-1", "case", &[("name", "Alpha")], 0),
                obj("case-2", "case", &[("name", "Bravo")], 0),
            ],
        )]),
    };
    let plan = formulate_plan("list the case", &summary(), &unclassified());
    let g = ground(&plan, &mut src, &ctx(), 1);

    assert_eq!(g.len(), 2);
    let cites = g.citations();
    assert_eq!(
        cites,
        vec!["obj:case-1".to_string(), "obj:case-2".to_string()]
    );
    // Relevance is monotonically non-increasing (first retrieved is most relevant).
    assert!(g.facts[0].relevance >= g.facts[1].relevance);
    // The rendered block carries the citation ids.
    assert!(g.render().contains("[obj:case-1]"));
    assert!(g.render().contains("name: Alpha"));
    // The retrieval's audit seq was captured.
    assert_eq!(g.audit_seqs, vec![7]);
}
