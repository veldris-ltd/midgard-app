//! AC-F-4 — Given a curated dataset, when projected, then the corresponding Midgard
//! objects/links appear with markings propagated. [FR-F-4] [NFR-SEC-2]

use std::collections::BTreeSet;

use fabric::store::{InMemoryStore, ObjectStore};
use fabric::value::Value;
use forge_dataset_store::{Column, ColumnType, DataValue, Dataset, Schema};
use forge_projection::{
    project_links, project_objects, LinkMapping, ObjectMapping, ProjectionError, PropertyMapping,
};
use policy::{ClassificationLevel, Compartment, Marking, Purpose};

fn secret_marking() -> Marking {
    let mut compartments = BTreeSet::new();
    compartments.insert(Compartment("OPS".into()));
    Marking {
        level: ClassificationLevel::SECRET,
        compartments,
    }
}

fn purposes() -> BTreeSet<Purpose> {
    let mut p = BTreeSet::new();
    p.insert(Purpose("analytics".into()));
    p
}

fn curated() -> Dataset {
    Dataset::new(
        Schema::new(vec![
            Column::new("site_id", ColumnType::Text),
            Column::new("units", ColumnType::Int),
            Column::nullable("note", ColumnType::Text),
        ]),
        vec![
            vec![
                DataValue::Text("site-emea".into()),
                DataValue::Int(120),
                DataValue::Null,
            ],
            vec![
                DataValue::Text("site-amer".into()),
                DataValue::Int(340),
                DataValue::Text("q4 spike".into()),
            ],
        ],
    )
    .unwrap()
}

#[test]
fn objects_are_projected_with_markings_propagated() {
    let mapping = ObjectMapping::new(
        "Site",
        "site_id",
        vec![
            PropertyMapping::new("units", "units"),
            PropertyMapping::new("note", "note"),
        ],
        secret_marking(),
        purposes(),
    );
    let mut store = InMemoryStore::new();

    let report = project_objects(&curated(), &mapping, &mut store).unwrap();
    assert_eq!(report.count, 2);
    assert_eq!(report.type_id, "Site");

    // Object 1 appears with typed properties and the propagated marking.
    let o1 = store.get_object("site-emea").unwrap();
    assert_eq!(o1.type_id, "Site");
    assert_eq!(o1.properties.get("units"), Some(&Value::Int(120)));
    assert!(!o1.properties.contains_key("note")); // null omitted
    assert_eq!(o1.marking, secret_marking()); // marking propagated [NFR-SEC-2]
    assert!(o1.allowed_purposes.contains(&Purpose("analytics".into())));

    let o2 = store.get_object("site-amer").unwrap();
    assert_eq!(
        o2.properties.get("note"),
        Some(&Value::Text("q4 spike".into()))
    );
    assert_eq!(o2.marking.level, ClassificationLevel::SECRET);

    // Every projected object carries the marking — none laundered away.
    for id in ["site-emea", "site-amer"] {
        assert_eq!(store.get_object(id).unwrap().marking, secret_marking());
    }
}

#[test]
fn links_are_projected_between_id_columns() {
    // A dataset of edges: parent_site -> child_site.
    let edges = Dataset::new(
        Schema::new(vec![
            Column::new("parent", ColumnType::Text),
            Column::new("child", ColumnType::Text),
        ]),
        vec![vec![
            DataValue::Text("site-emea".into()),
            DataValue::Text("site-amer".into()),
        ]],
    )
    .unwrap();

    // Seed the objects so neighbours resolve.
    let mut store = InMemoryStore::new();
    let obj_mapping = ObjectMapping::new("Site", "site_id", vec![], secret_marking(), purposes());
    project_objects(&curated(), &obj_mapping, &mut store).unwrap();

    let link_mapping = LinkMapping::new("reports_to", "parent", "child");
    let report = project_links(&edges, &link_mapping, &mut store).unwrap();
    assert_eq!(report.count, 1);

    let neighbours = store.neighbors("site-emea", "reports_to");
    assert_eq!(neighbours.len(), 1);
    assert_eq!(neighbours[0].id, "site-amer");
}

#[test]
fn unmapped_column_fails_closed() {
    let mapping = ObjectMapping::new("Site", "missing_id", vec![], secret_marking(), purposes());
    let mut store = InMemoryStore::new();
    assert_eq!(
        project_objects(&curated(), &mapping, &mut store),
        Err(ProjectionError::UnknownColumn("missing_id".into()))
    );
}

#[test]
fn bytes_value_has_no_fabric_representation() {
    let ds = Dataset::new(
        Schema::new(vec![
            Column::new("id", ColumnType::Text),
            Column::new("blob", ColumnType::Bytes),
        ]),
        vec![vec![
            DataValue::Text("a".into()),
            DataValue::Bytes(vec![1, 2, 3]),
        ]],
    )
    .unwrap();
    let mapping = ObjectMapping::new(
        "Thing",
        "id",
        vec![PropertyMapping::new("blob", "blob")],
        secret_marking(),
        purposes(),
    );
    let mut store = InMemoryStore::new();
    assert!(matches!(
        project_objects(&ds, &mapping, &mut store),
        Err(ProjectionError::UnrepresentableValue { .. })
    ));
}
