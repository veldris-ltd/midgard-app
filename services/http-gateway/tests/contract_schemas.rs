//! The published file-format contracts and the parsers agree. [AC-RF-CA-13c] [DD-1] [DD-6]
//!
//! `contracts/schema/ontology.v1.schema.json` and `import-record.v1.schema.json` are
//! what another tool is told to produce. The gateway does not read them: it has its
//! own strict parsers. So the two could drift, and a file that validates against the
//! schema could be refused at start. These tests hold them together:
//!
//! - the keys each schema allows are exactly the keys the parser allows, compared
//!   against the parser's own constants;
//! - every enumerated value in a schema is accepted by the parser;
//! - every fixture under `contracts/fixtures` gets the verdict its directory names,
//!   from the schema and from the parser: `valid` is accepted by both, `invalid` is
//!   refused by both, and `beyond-schema` holds the rules a schema cannot state
//!   (references and uniqueness), which the schema accepts and the parser refuses;
//! - an unknown key added to any object of a valid ontology is refused by both.
//!
//! The validator below covers the JSON Schema keywords these two schemas use and
//! panics on any other, so a schema that starts using a new keyword fails here until
//! the validator learns it.
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use http_gateway::import_file::{parse_import, LINK_RECORD_KEYS, OBJECT_RECORD_KEYS};
use http_gateway::json::{parse, Json};
use http_gateway::ontology_file::{
    parse_ontology, Ontology, ACTION_KEYS, CYCLE_PROPERTY_KEYS, LINK_TYPE_KEYS, OBJECT_TYPE_KEYS,
    PROPERTY_KEYS, RETENTION_KEYS, ROOT_KEYS, SET_PROPERTY_KEYS,
};
use service_runtime::marking::BANDS;

// ---- files ------------------------------------------------------------------------------

fn contracts() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn schema(name: &str) -> Json {
    let path = contracts().join("schema").join(name);
    parse(&read(&path)).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The fixtures in one directory, sorted, as (file name, contents).
fn fixtures(kind: &str, verdict: &str) -> Vec<(String, String)> {
    let dir = contracts().join("fixtures").join(kind).join(verdict);
    let mut found: Vec<(String, String)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .map(|path| {
            (
                path.file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned(),
                read(&path),
            )
        })
        .collect();
    found.sort();
    assert!(!found.is_empty(), "{} holds no fixtures", dir.display());
    found
}

// ---- a validator for the keywords these schemas use ---------------------------------------

const IDENTIFIER_PATTERN: &str = "^[A-Za-z0-9_-]+$";
const NOT_BLANK_PATTERN: &str = "\\S";
const ANNOTATIONS: &[&str] = &["$schema", "$id", "$defs", "title", "description", "default"];

fn same(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Int(x), Json::Num(y)) | (Json::Num(y), Json::Int(x)) => (*x as f64) == *y,
        _ => a == b,
    }
}

fn is_type(value: &Json, name: &str) -> bool {
    match (name, value) {
        ("object", Json::Obj(_))
        | ("array", Json::Arr(_))
        | ("string", Json::Str(_))
        | ("boolean", Json::Bool(_))
        | ("null", Json::Null)
        | ("integer", Json::Int(_))
        | ("number", Json::Int(_) | Json::Num(_)) => true,
        ("integer", Json::Num(n)) => n.fract() == 0.0,
        ("object" | "array" | "string" | "boolean" | "null" | "integer" | "number", _) => false,
        (other, _) => panic!("the validator does not know the type \"{other}\""),
    }
}

fn resolve<'a>(root: &'a Json, reference: &str) -> &'a Json {
    let name = reference.strip_prefix("#/$defs/").unwrap_or_else(|| {
        panic!("the validator follows only #/$defs references, not {reference}")
    });
    root.get("$defs")
        .and_then(|defs| defs.get(name))
        .unwrap_or_else(|| panic!("{reference} is not defined"))
}

fn validate(root: &Json, schema: &Json, value: &Json, at: &str) -> Result<(), String> {
    let Json::Obj(keywords) = schema else {
        panic!("a schema must be an object at {at}")
    };
    for (keyword, arg) in keywords {
        match (keyword.as_str(), arg) {
            (k, _) if ANNOTATIONS.contains(&k) => {}
            ("$ref", Json::Str(reference)) => validate(root, resolve(root, reference), value, at)?,
            ("const", expected) => {
                if !same(expected, value) {
                    return Err(format!("{at}: must be {}", expected.encode()));
                }
            }
            ("enum", Json::Arr(allowed)) => {
                if !allowed.iter().any(|a| same(a, value)) {
                    return Err(format!("{at}: {} is not an allowed value", value.encode()));
                }
            }
            ("type", Json::Str(name)) => {
                if !is_type(value, name) {
                    return Err(format!("{at}: must be of type {name}"));
                }
            }
            ("type", Json::Arr(names)) => {
                let names: Vec<&str> = names.iter().filter_map(Json::as_str).collect();
                if !names.iter().any(|name| is_type(value, name)) {
                    return Err(format!("{at}: must be one of the types {names:?}"));
                }
            }
            ("pattern", Json::Str(pattern)) => {
                if let Json::Str(s) = value {
                    let ok = match pattern.as_str() {
                        IDENTIFIER_PATTERN => {
                            !s.is_empty()
                                && s.chars()
                                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                        }
                        NOT_BLANK_PATTERN => s.chars().any(|c| !c.is_whitespace()),
                        other => panic!("the validator does not know the pattern {other}"),
                    };
                    if !ok {
                        return Err(format!("{at}: \"{s}\" does not match {pattern}"));
                    }
                }
            }
            ("required", Json::Arr(names)) => {
                if let Json::Obj(_) = value {
                    for name in names.iter().filter_map(Json::as_str) {
                        if value.get(name).is_none() {
                            return Err(format!("{at}: missing \"{name}\""));
                        }
                    }
                }
            }
            ("properties", Json::Obj(properties)) => {
                for (name, sub) in properties {
                    if let Some(child) = value.get(name) {
                        validate(root, sub, child, &format!("{at}.{name}"))?;
                    }
                }
            }
            ("additionalProperties", rule) => {
                if let Json::Obj(fields) = value {
                    let declared: Vec<&str> = match schema.get("properties") {
                        Some(Json::Obj(properties)) => {
                            properties.iter().map(|(k, _)| k.as_str()).collect()
                        }
                        _ => Vec::new(),
                    };
                    for (name, child) in fields {
                        if declared.contains(&name.as_str()) {
                            continue;
                        }
                        match rule {
                            Json::Bool(false) => {
                                return Err(format!("{at}: unknown key \"{name}\""))
                            }
                            Json::Bool(true) => {}
                            sub => validate(root, sub, child, &format!("{at}.{name}"))?,
                        }
                    }
                }
            }
            ("items", sub) => {
                if let Json::Arr(items) = value {
                    for (i, item) in items.iter().enumerate() {
                        validate(root, sub, item, &format!("{at}[{i}]"))?;
                    }
                }
            }
            ("minItems", Json::Int(min)) => {
                if let Json::Arr(items) = value {
                    if (items.len() as i64) < *min {
                        return Err(format!("{at}: needs at least {min} item(s)"));
                    }
                }
            }
            ("uniqueItems", Json::Bool(true)) => {
                if let Json::Arr(items) = value {
                    for (i, item) in items.iter().enumerate() {
                        if items[..i].iter().any(|earlier| same(earlier, item)) {
                            return Err(format!("{at}: {} appears twice", item.encode()));
                        }
                    }
                }
            }
            ("oneOf", Json::Arr(options)) => {
                let matches = options
                    .iter()
                    .filter(|option| validate(root, option, value, at).is_ok())
                    .count();
                if matches != 1 {
                    return Err(format!(
                        "{at}: matches {matches} of the alternatives, not one"
                    ));
                }
            }
            (other, _) => panic!("the validator does not know the keyword \"{other}\" at {at}"),
        }
    }
    Ok(())
}

fn check(schema: &Json, text: &str) -> Result<(), String> {
    let value = parse(text).map_err(|e| format!("not valid JSON: {e}"))?;
    validate(schema, schema, &value, "$")
}

/// Every non-blank line of a JSON Lines file against the record schema.
fn check_lines(schema: &Json, text: &str) -> Result<(), String> {
    for (i, line) in text.lines().enumerate() {
        if !line.trim().is_empty() {
            check(schema, line).map_err(|e| format!("line {}: {e}", i + 1))?;
        }
    }
    Ok(())
}

// ---- the schemas' keys and values are the parsers' -----------------------------------------

fn keys(schema: &Json, def: Option<&str>) -> BTreeSet<String> {
    let node = match def {
        Some(name) => resolve(schema, &format!("#/$defs/{name}")),
        None => schema,
    };
    assert_eq!(
        node.get("additionalProperties"),
        Some(&Json::Bool(false)),
        "{def:?} must refuse unknown keys, as the parser does"
    );
    match node.get("properties") {
        Some(Json::Obj(properties)) => properties.iter().map(|(k, _)| k.clone()).collect(),
        _ => panic!("{def:?} declares no properties"),
    }
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

fn enumerated(schema: &Json, def: &str, property: Option<&str>) -> Vec<String> {
    let mut node = resolve(schema, &format!("#/$defs/{def}"));
    if let Some(property) = property {
        node = node
            .get("properties")
            .and_then(|p| p.get(property))
            .unwrap_or_else(|| panic!("{def} has no property {property}"));
    }
    match node.get("enum") {
        Some(Json::Arr(values)) => values
            .iter()
            .map(|v| v.as_str().expect("enum of strings").to_string())
            .collect(),
        _ => panic!("{def}.{property:?} is not an enum"),
    }
}

#[test]
fn the_ontology_schema_allows_exactly_the_keys_the_parser_allows() {
    let schema = schema("ontology.v1.schema.json");
    assert_eq!(keys(&schema, None), set(ROOT_KEYS));
    assert_eq!(keys(&schema, Some("objectType")), set(OBJECT_TYPE_KEYS));
    assert_eq!(keys(&schema, Some("property")), set(PROPERTY_KEYS));
    assert_eq!(keys(&schema, Some("retention")), set(RETENTION_KEYS));
    assert_eq!(keys(&schema, Some("linkType")), set(LINK_TYPE_KEYS));
    assert_eq!(keys(&schema, Some("action")), set(ACTION_KEYS));
    assert_eq!(keys(&schema, Some("setProperty")), set(SET_PROPERTY_KEYS));
    assert_eq!(
        keys(&schema, Some("cycleProperty")),
        set(CYCLE_PROPERTY_KEYS)
    );
}

#[test]
fn the_import_record_schema_allows_exactly_the_keys_the_parser_allows() {
    let schema = schema("import-record.v1.schema.json");
    assert_eq!(keys(&schema, Some("objectRecord")), set(OBJECT_RECORD_KEYS));
    assert_eq!(keys(&schema, Some("linkRecord")), set(LINK_RECORD_KEYS));
}

#[test]
fn every_enumerated_value_is_one_the_parser_accepts() {
    let ontology = schema("ontology.v1.schema.json");
    let record = schema("import-record.v1.schema.json");

    // Marking bands are the runtime's own list, in both schemas.
    assert_eq!(enumerated(&ontology, "band", None), BANDS);
    assert_eq!(enumerated(&record, "band", None), BANDS);

    for value_type in enumerated(&ontology, "property", Some("type")) {
        let file = format!(
            r#"{{"version":1,"objectTypes":[{{"id":"t","title":"T","properties":[{{"name":"p","type":"{value_type}"}}]}}]}}"#
        );
        check(&ontology, &file).expect("the schema accepts its own value");
        parse_ontology(&file)
            .unwrap_or_else(|e| panic!("the parser refuses property type {value_type}: {e}"));
    }
    for cardinality in enumerated(&ontology, "linkType", Some("cardinality")) {
        let file = format!(
            r#"{{"version":1,"objectTypes":[{{"id":"t","title":"T","properties":[]}}],"linkTypes":[{{"id":"l","name":"l","from":"t","to":"t","cardinality":"{cardinality}"}}]}}"#
        );
        check(&ontology, &file).expect("the schema accepts its own value");
        parse_ontology(&file)
            .unwrap_or_else(|e| panic!("the parser refuses cardinality {cardinality}: {e}"));
    }
    for band in BANDS {
        let file = format!(
            r#"{{"version":1,"objectTypes":[{{"id":"t","title":"T","properties":[{{"name":"p","type":"text"}}]}}],"actions":[{{"id":"a","name":"a","title":"A","objectType":"t","marking":"{band}","purposes":["x"],"effect":{{"kind":"set-property","property":"p","value":"v"}}}}]}}"#
        );
        check(&ontology, &file).expect("the schema accepts its own value");
        parse_ontology(&file).unwrap_or_else(|e| panic!("the parser refuses marking {band}: {e}"));
    }
}

// ---- fixtures: one verdict, from both -------------------------------------------------------

#[test]
fn ontology_fixtures_get_the_same_verdict_from_the_schema_and_the_parser() {
    let schema = schema("ontology.v1.schema.json");
    for (name, text) in fixtures("ontology", "valid") {
        check(&schema, &text)
            .unwrap_or_else(|e| panic!("valid/{name}: the schema refuses it: {e}"));
        parse_ontology(&text)
            .unwrap_or_else(|e| panic!("valid/{name}: the parser refuses it: {e}"));
    }
    for (name, text) in fixtures("ontology", "invalid") {
        assert!(
            check(&schema, &text).is_err(),
            "invalid/{name}: the schema accepts it"
        );
        assert!(
            parse_ontology(&text).is_err(),
            "invalid/{name}: the parser accepts it"
        );
    }
    for (name, text) in fixtures("ontology", "beyond-schema") {
        check(&schema, &text)
            .unwrap_or_else(|e| panic!("beyond-schema/{name}: the schema refuses it: {e}"));
        assert!(
            parse_ontology(&text).is_err(),
            "beyond-schema/{name}: the parser accepts it"
        );
    }
}

fn full_ontology() -> (String, Ontology) {
    let text = read(&contracts().join("fixtures/ontology/valid/full.json"));
    let ontology = parse_ontology(&text).expect("valid/full.json parses");
    (text, ontology)
}

#[test]
fn import_fixtures_get_the_same_verdict_from_the_schema_and_the_importer() {
    let schema = schema("import-record.v1.schema.json");
    let (_, ontology) = full_ontology();
    for (name, text) in fixtures("import", "valid") {
        check_lines(&schema, &text)
            .unwrap_or_else(|e| panic!("valid/{name}: the schema refuses it: {e}"));
        parse_import(&text, &ontology)
            .unwrap_or_else(|e| panic!("valid/{name}: the importer refuses it: {e}"));
    }
    for (name, text) in fixtures("import", "invalid") {
        assert!(
            check_lines(&schema, &text).is_err(),
            "invalid/{name}: the schema accepts it"
        );
        assert!(
            parse_import(&text, &ontology).is_err(),
            "invalid/{name}: the importer accepts it"
        );
    }
    for (name, text) in fixtures("import", "beyond-schema") {
        check_lines(&schema, &text)
            .unwrap_or_else(|e| panic!("beyond-schema/{name}: the schema refuses it: {e}"));
        assert!(
            parse_import(&text, &ontology).is_err(),
            "beyond-schema/{name}: the importer accepts it"
        );
    }
}

// ---- an unknown key anywhere is refused by both ----------------------------------------------

/// Every copy of `value` that differs by one extra key on one of its objects.
fn with_an_unknown_key(value: &Json) -> Vec<Json> {
    let mut variants = Vec::new();
    match value {
        Json::Obj(fields) => {
            let mut extended = fields.clone();
            extended.push(("x-unknown".to_string(), Json::Bool(true)));
            variants.push(Json::Obj(extended));
            for (i, (_, child)) in fields.iter().enumerate() {
                for changed in with_an_unknown_key(child) {
                    let mut copy = fields.clone();
                    copy[i].1 = changed;
                    variants.push(Json::Obj(copy));
                }
            }
        }
        Json::Arr(items) => {
            for (i, child) in items.iter().enumerate() {
                for changed in with_an_unknown_key(child) {
                    let mut copy = items.clone();
                    copy[i] = changed;
                    variants.push(Json::Arr(copy));
                }
            }
        }
        _ => {}
    }
    variants
}

#[test]
fn an_unknown_key_on_any_object_of_an_ontology_is_refused_by_both() {
    let schema = schema("ontology.v1.schema.json");
    let (text, _) = full_ontology();
    let variants = with_an_unknown_key(&parse(&text).expect("valid/full.json is JSON"));
    // The file, 2 object types, 1 retention window, 8 properties, 1 link type, 2 actions
    // and 2 effects.
    assert_eq!(variants.len(), 17);
    for variant in variants {
        let text = variant.encode();
        assert!(check(&schema, &text).is_err(), "the schema accepts {text}");
        assert!(parse_ontology(&text).is_err(), "the parser accepts {text}");
    }
}

#[test]
fn the_full_fixture_exercises_every_key_the_schema_declares() {
    let (text, _) = full_ontology();
    for key in [
        ROOT_KEYS,
        OBJECT_TYPE_KEYS,
        PROPERTY_KEYS,
        RETENTION_KEYS,
        LINK_TYPE_KEYS,
        ACTION_KEYS,
        SET_PROPERTY_KEYS,
        CYCLE_PROPERTY_KEYS,
    ]
    .concat()
    {
        assert!(
            text.contains(&format!("\"{key}\"")),
            "valid/full.json never uses \"{key}\""
        );
    }
}

#[test]
fn the_demonstration_ontology_meets_the_published_schema() {
    let schema = schema("ontology.v1.schema.json");
    let text = read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/compose/core-ontology.json"),
    );
    check(&schema, &text).expect("deploy/compose/core-ontology.json meets the schema");
    parse_ontology(&text).expect("and the gateway accepts it");
}
