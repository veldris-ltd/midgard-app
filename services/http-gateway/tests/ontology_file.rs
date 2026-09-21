//! The ontology file declares exactly the schema an enclosure registers: a full file
//! parses field for field, it registers into a real enclosure's fabric and catalogue,
//! and a file that is malformed or inconsistent with itself is refused whole.
//! [DD-1] [NFR-AI-1]
#![cfg(feature = "backend-fips")]

use fabric::{Cardinality, ValueType};
use governance::Governance;
use http_gateway::enclosure::{level_of, Enclosure};
use http_gateway::ontology_file::{parse_ontology, ActionEffect};
use policy::{Compartment, DefaultPdp, PolicySet, Purpose};
use pqc_agility::OsRng;

const FULL: &str = r#"{
  "version": 1,
  "objectTypes": [
    { "id": "aircraft", "title": "Aircraft",
      "properties": [ { "name": "tail", "type": "text", "required": true },
                      { "name": "status", "type": "text" },
                      { "name": "flight_hours", "type": "int" } ] },
    { "id": "facility", "title": "Facility",
      "properties": [ { "name": "name", "type": "text", "required": true },
                      { "name": "status", "type": "text" } ] }
  ],
  "linkTypes": [
    { "id": "aircraft_base", "name": "based at", "from": "aircraft", "to": "facility",
      "cardinality": "one-to-one" }
  ],
  "actions": [
    { "id": "ground_aircraft", "name": "groundAircraft", "title": "Ground aircraft",
      "objectType": "aircraft", "marking": "confidential", "compartments": ["OPS-N"],
      "purposes": ["operations"],
      "effect": { "kind": "set-property", "property": "status", "value": "grounded" } },
    { "id": "change_facility_status", "name": "changeFacilityStatus", "title": "Change status",
      "objectType": "facility", "humanGated": false, "marking": "restricted",
      "purposes": ["operations", "maintenance"],
      "effect": { "kind": "cycle-property", "property": "status",
                  "values": ["operational", "degraded"] } }
  ]
}"#;

#[test]
fn a_full_file_parses_field_for_field() {
    let o = parse_ontology(FULL).expect("parses");

    assert_eq!(o.object_types.len(), 2);
    let aircraft = &o.object_types[0];
    assert_eq!(
        (aircraft.id.as_str(), aircraft.title.as_str()),
        ("aircraft", "Aircraft")
    );
    let tail = aircraft.property("tail").expect("tail");
    assert_eq!((tail.value_type, tail.required), (ValueType::Text, true));
    let hours = aircraft.property("flight_hours").expect("flight_hours");
    assert_eq!((hours.value_type, hours.required), (ValueType::Int, false));

    let link = &o.link_types[0];
    assert_eq!(
        (
            link.api_name.as_str(),
            link.source_type.as_str(),
            link.target_type.as_str()
        ),
        ("based at", "aircraft", "facility")
    );
    assert_eq!(link.cardinality, Cardinality::OneToOne);

    let ground = &o.actions[0];
    assert_eq!(ground.api_name, "groundAircraft");
    assert!(
        ground.human_gated,
        "an action is human-gated unless the file says otherwise"
    );
    assert_eq!(ground.marking.level, level_of("confidential").unwrap());
    assert!(ground
        .marking
        .compartments
        .contains(&Compartment("OPS-N".into())));
    assert_eq!(
        ground.effect,
        ActionEffect::SetProperty {
            property: "status".into(),
            value: "grounded".into()
        }
    );

    let cycle = &o.actions[1];
    assert!(!cycle.human_gated);
    assert!(cycle.purposes.contains(&Purpose("maintenance".into())));
    assert_eq!(
        cycle.effect,
        ActionEffect::CycleProperty {
            property: "status".into(),
            values: vec!["operational".into(), "degraded".into()]
        }
    );
}

#[test]
fn object_types_alone_are_a_valid_ontology() {
    let o = parse_ontology(
        r#"{"version":1,"objectTypes":[{"id":"case","title":"Case","properties":[]}]}"#,
    )
    .expect("parses");
    assert_eq!(o.object_types.len(), 1);
    assert!(o.link_types.is_empty() && o.actions.is_empty());
}

#[test]
fn it_registers_into_an_enclosure() {
    let o = parse_ontology(FULL).expect("parses");
    let governance = Governance::new(DefaultPdp::new(PolicySet::default()), &mut OsRng).unwrap();
    let mut enclosure = Enclosure::new(governance);
    enclosure
        .load_ontology(&o, 1_800_000_000)
        .expect("registers");

    let registry = enclosure.fabric().registry();
    assert!(registry.object_type("aircraft").is_some());
    assert!(registry.object_type("facility").is_some());
    assert!(registry.link_type("aircraft_base").is_some());
    let ground = registry
        .action_type("ground_aircraft")
        .expect("action registered");
    assert_eq!(ground.api_name, "groundAircraft");
    assert_eq!(
        ground.parameters.len(),
        1,
        "the edge-resolved targets parameter"
    );

    // Registering the same ontology twice is refused, not silently doubled.
    assert!(enclosure.load_ontology(&o, 1_800_000_000).is_err());
}

#[test]
fn a_malformed_or_inconsistent_file_is_refused() {
    let with = |types: &str, links: &str, actions: &str| {
        format!(
            r#"{{"version":1,"objectTypes":[{types}],"linkTypes":[{links}],"actions":[{actions}]}}"#
        )
    };
    let aircraft = r#"{"id":"aircraft","title":"Aircraft","properties":[{"name":"status","type":"text"},{"name":"hours","type":"int"}]}"#;
    let action = |extra: &str, effect: &str| {
        format!(
            r#"{{"id":"a","name":"act","title":"Act","objectType":"aircraft","marking":"internal","purposes":["ops"],"effect":{effect}{extra}}}"#
        )
    };
    let set_status = r#"{"kind":"set-property","property":"status","value":"x"}"#;
    assert!(
        parse_ontology(&with(aircraft, "", &action("", set_status))).is_ok(),
        "the baseline loads"
    );

    let cases: Vec<(&str, String)> = vec![
        ("not JSON", "{".into()),
        ("missing version", r#"{"objectTypes":[]}"#.into()),
        ("future version", r#"{"version":2,"objectTypes":[]}"#.into()),
        ("missing objectTypes", r#"{"version":1}"#.into()),
        (
            "unknown top-level key",
            r#"{"version":1,"objectTypes":[],"views":[]}"#.into(),
        ),
        (
            "repeated top-level key",
            r#"{"version":1,"version":1,"objectTypes":[]}"#.into(),
        ),
        (
            "duplicate object type",
            with(&format!("{aircraft},{aircraft}"), "", ""),
        ),
        (
            "slash in an id",
            with(r#"{"id":"air/craft","title":"A","properties":[]}"#, "", ""),
        ),
        (
            "unknown property type",
            with(
                r#"{"id":"t","title":"T","properties":[{"name":"p","type":"string"}]}"#,
                "",
                "",
            ),
        ),
        (
            "duplicate property",
            with(
                r#"{"id":"t","title":"T","properties":[{"name":"p","type":"text"},{"name":"p","type":"int"}]}"#,
                "",
                "",
            ),
        ),
        (
            "misspelt property key",
            with(
                r#"{"id":"t","title":"T","properties":[{"name":"p","typ":"text"}]}"#,
                "",
                "",
            ),
        ),
        (
            "link to an undeclared type",
            with(
                aircraft,
                r#"{"id":"l","name":"to","from":"aircraft","to":"facility","cardinality":"one-to-one"}"#,
                "",
            ),
        ),
        (
            "unknown cardinality",
            with(
                aircraft,
                r#"{"id":"l","name":"self","from":"aircraft","to":"aircraft","cardinality":"some"}"#,
                "",
            ),
        ),
        (
            "action on an undeclared type",
            with(
                aircraft,
                "",
                r#"{"id":"a","name":"act","title":"Act","objectType":"ship","marking":"internal","purposes":["ops"],"effect":{"kind":"set-property","property":"status","value":"x"}}"#,
            ),
        ),
        (
            "effect on an undeclared property",
            with(
                aircraft,
                "",
                &action(
                    "",
                    r#"{"kind":"set-property","property":"colour","value":"x"}"#,
                ),
            ),
        ),
        (
            "effect on a non-text property",
            with(
                aircraft,
                "",
                &action(
                    "",
                    r#"{"kind":"set-property","property":"hours","value":"1"}"#,
                ),
            ),
        ),
        (
            "a cycle of one value",
            with(
                aircraft,
                "",
                &action(
                    "",
                    r#"{"kind":"cycle-property","property":"status","values":["x"]}"#,
                ),
            ),
        ),
        (
            "unknown effect kind",
            with(
                aircraft,
                "",
                &action("", r#"{"kind":"delete","property":"status"}"#),
            ),
        ),
        (
            "unknown effect key",
            with(
                aircraft,
                "",
                &action(
                    "",
                    r#"{"kind":"set-property","property":"status","value":"x","to":"y"}"#,
                ),
            ),
        ),
        (
            "unknown marking band",
            with(
                aircraft,
                "",
                &action("", set_status).replace(r#""internal""#, r#""secret""#),
            ),
        ),
        (
            "no purposes",
            with(
                aircraft,
                "",
                &action("", set_status).replace(r#"["ops"]"#, "[]"),
            ),
        ),
        (
            "non-boolean humanGated",
            with(aircraft, "", &action(r#","humanGated":"no""#, set_status)),
        ),
        (
            "duplicate action name",
            with(
                aircraft,
                "",
                &format!(
                    "{},{}",
                    action("", set_status),
                    action("", set_status).replacen(r#""id":"a""#, r#""id":"b""#, 1)
                ),
            ),
        ),
    ];
    for (label, text) in &cases {
        let result = parse_ontology(text);
        assert!(result.is_err(), "{label} must be refused: {text}");
        assert!(
            result
                .as_ref()
                .err()
                .unwrap()
                .starts_with("ontology file refused:"),
            "{label}: {result:?}"
        );
    }
}
