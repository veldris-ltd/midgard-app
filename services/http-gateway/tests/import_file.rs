//! An import file carries exactly the objects and links it declares, typed and marked as
//! the ontology requires. A file with any bad line is refused whole, naming the line,
//! and an import never replaces an object or links to one that does not exist.
//! [DD-1] [RC-10]

use fabric::Value;
use http_gateway::enclosure::level_of;
use http_gateway::import_file::parse_import;
use http_gateway::ontology_file::{parse_ontology, Ontology};
use policy::{Compartment, Purpose};

fn ontology() -> Ontology {
    parse_ontology(
        r#"{
          "version": 1,
          "objectTypes": [
            { "id": "aircraft", "title": "Aircraft",
              "properties": [ { "name": "tail", "type": "text", "required": true },
                              { "name": "hours", "type": "int" },
                              { "name": "fuel", "type": "float" },
                              { "name": "airworthy", "type": "bool" },
                              { "name": "serviced", "type": "timestamp" } ] },
            { "id": "facility", "title": "Facility",
              "properties": [ { "name": "name", "type": "text", "required": true } ] }
          ],
          "linkTypes": [
            { "id": "aircraft_base", "name": "based at", "from": "aircraft", "to": "facility",
              "cardinality": "one-to-one" }
          ]
        }"#,
    )
    .expect("the test ontology parses")
}

const GOOD: &str = r#"
{"object":"aircraft","key":"AC-1","marking":"confidential","compartments":["OPS-N"],"purposes":["operations"],"properties":{"tail":"MG-001","hours":6412,"fuel":0.75,"airworthy":true,"serviced":1700000000}}

{"object":"facility","key":"FAC-N","marking":"restricted","purposes":["operations","maintenance"],"properties":{"name":"North"}}
{"link":"aircraft_base","from":"AC-1","to":"FAC-N"}
"#;

#[test]
fn a_good_file_imports_record_for_record() {
    let import = parse_import(GOOD, &ontology()).expect("parses");
    assert_eq!(import.objects.len(), 2);
    assert_eq!(import.links.len(), 1);

    let aircraft = &import.objects[0];
    assert_eq!(aircraft.id, "aircraft/AC-1");
    assert_eq!(aircraft.type_id, "aircraft");
    assert_eq!(aircraft.properties["tail"], Value::Text("MG-001".into()));
    assert_eq!(aircraft.properties["hours"], Value::Int(6412));
    assert_eq!(aircraft.properties["fuel"], Value::Float(0.75));
    assert_eq!(aircraft.properties["airworthy"], Value::Bool(true));
    assert_eq!(
        aircraft.properties["serviced"],
        Value::Timestamp(1_700_000_000)
    );
    assert_eq!(aircraft.marking.level, level_of("confidential").unwrap());
    assert!(aircraft
        .marking
        .compartments
        .contains(&Compartment("OPS-N".into())));
    assert!(import.objects[1]
        .allowed_purposes
        .contains(&Purpose("maintenance".into())));

    let link = &import.links[0];
    assert_eq!(
        (
            link.link_type.as_str(),
            link.source_id.as_str(),
            link.target_id.as_str()
        ),
        ("aircraft_base", "aircraft/AC-1", "facility/FAC-N")
    );
}

#[test]
fn a_bad_line_refuses_the_whole_file_and_names_it() {
    let aircraft = |props: &str, extra: &str| {
        format!(
            r#"{{"object":"aircraft","key":"AC-2","marking":"internal","purposes":["operations"],"properties":{props}{extra}}}"#
        )
    };
    let cases: Vec<(&str, String)> = vec![
        ("not JSON", "{".into()),
        ("neither object nor link", r#"{"key":"AC-2"}"#.into()),
        (
            "both object and link",
            r#"{"object":"aircraft","link":"aircraft_base"}"#.into(),
        ),
        (
            "undeclared object type",
            r#"{"object":"ship","key":"S-1","marking":"internal","purposes":["operations"]}"#
                .into(),
        ),
        (
            "undeclared property",
            aircraft(r#"{"tail":"T","colour":"red"}"#, ""),
        ),
        (
            "string for an int",
            aircraft(r#"{"tail":"T","hours":"12"}"#, ""),
        ),
        (
            "float for an int",
            aircraft(r#"{"tail":"T","hours":1.5}"#, ""),
        ),
        (
            "negative timestamp",
            aircraft(r#"{"tail":"T","serviced":-1}"#, ""),
        ),
        ("missing required property", aircraft(r#"{"hours":1}"#, "")),
        (
            "repeated property",
            aircraft(r#"{"tail":"T","tail":"U"}"#, ""),
        ),
        (
            "unknown key",
            aircraft(r#"{"tail":"T"}"#, r#","owner":"x""#),
        ),
        (
            "unknown marking band",
            aircraft(r#"{"tail":"T"}"#, "").replace(r#""internal""#, r#""secret""#),
        ),
        (
            "no purposes",
            aircraft(r#"{"tail":"T"}"#, "").replace(r#"["operations"]"#, "[]"),
        ),
        (
            "a key with a slash",
            aircraft(r#"{"tail":"T"}"#, "").replace("AC-2", "AC/2"),
        ),
        (
            "undeclared link type",
            r#"{"link":"aircraft_crew","from":"AC-1","to":"CR-1"}"#.into(),
        ),
    ];
    for (label, bad) in &cases {
        let text = format!("{GOOD}{bad}\n");
        let bad_line = text.lines().count();
        let result = parse_import(&text, &ontology());
        let error = result.expect_err(label);
        assert!(
            error.starts_with(&format!("import refused: line {bad_line}")),
            "{label}: {error}"
        );
    }

    let duplicate = format!(
        "{GOOD}{}\n",
        GOOD.lines().nth(1).expect("the aircraft line")
    );
    assert!(parse_import(&duplicate, &ontology())
        .expect_err("a repeated object")
        .contains("appears twice"));
}

#[test]
fn an_import_never_replaces_an_object_or_links_to_nothing() {
    let import = parse_import(GOOD, &ontology()).expect("parses");
    assert!(import.check_against(&|_| false).is_ok(), "an empty store");

    let clash = import
        .check_against(&|id| id == "aircraft/AC-1")
        .expect_err("aircraft/AC-1 already exists");
    assert!(clash.contains("already in the enclosure"), "{clash}");

    let dangling = parse_import(
        r#"{"link":"aircraft_base","from":"AC-9","to":"FAC-N"}"#,
        &ontology(),
    )
    .expect("parses on its own");
    assert!(dangling
        .check_against(&|id| id == "facility/FAC-N")
        .expect_err("AC-9 is nowhere")
        .contains("aircraft/AC-9"));
    assert!(
        dangling
            .check_against(&|id| id == "facility/FAC-N" || id == "aircraft/AC-9")
            .is_ok(),
        "both ends already in the store"
    );
}
