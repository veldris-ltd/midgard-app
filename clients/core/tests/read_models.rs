//! The generated console read models decode what the web consoles' mocks serve, and
//! refuse a shape they do not know. [FR-C-1] [AC-C-13]

use attest::json::parse;
use midgard_client_core::consoles::admin::AdminApproval;
use midgard_client_core::consoles::decode::{DecodeError, FromJson, KeyValues};
use midgard_client_core::consoles::heimdall::HeimdallTrack;
use midgard_client_core::consoles::midgard::MidgardObjectInstance;
use midgard_client_core::consoles::mimir::MimirTraceStep;

fn decode<T: FromJson>(text: &str) -> Result<T, DecodeError> {
    T::from_json(&parse(text).expect("valid json"), "$")
}

#[test]
fn an_admin_approval_decodes_with_its_optional_field_absent() {
    let a: AdminApproval = decode(
        r#"{"id":"APR-1","action":"Rotate KEK","subsystem":"kms","risk":4,"requester":"k.rao",
            "approver":"—","state":"awaiting","justification":"quarterly"}"#,
    )
    .unwrap();
    assert_eq!(a.risk, 4);
    assert_eq!(a.denied_reason, None);
}

#[test]
fn nested_lists_nulls_and_records_decode() {
    let t: HeimdallTrack = decode(
        r#"{"id":"E-0871","cls":"UNKNOWN","conf":0.54,"x":74,"y":34,"layer":"air","track":"anomalous",
            "hdg":197,"spd":388,"alt":"FL212","feeds":2,
            "options":[{"rank":1,"title":"Re-task","why":"gain","cites":["Entity/E-0871"]}]}"#,
    )
    .unwrap();
    assert_eq!(t.options.as_ref().map(Vec::len), Some(1));

    // MimirTraceStep (sdk/ts/src/products/mimir.ts): `ms: number | null`, `ok: boolean | null`.
    let s: MimirTraceStep =
        decode(r#"{"kind":"plan","label":"p","detail":"d","ms":null,"ok":null}"#).unwrap();
    assert_eq!((s.ms, s.ok), (None, None));

    let o: MidgardObjectInstance = decode(
        r#"{"id":"aircraft/1000","typeId":"aircraft","propertyValues":{"status":"standby","updated":"2m ago"},
            "markings":[{"id":"mk-confidential","name":"CONFIDENTIAL","level":"confidential","compartments":["OPS-N"]}]}"#,
    )
    .unwrap();
    assert_eq!(
        o.property_values,
        KeyValues(vec![
            ("status".into(), "standby".into()),
            ("updated".into(), "2m ago".into())
        ])
    );
}

#[test]
fn an_unknown_key_or_a_wrong_type_is_refused_with_its_path() {
    let e = decode::<AdminApproval>(
        r#"{"id":"APR-1","action":"a","subsystem":"s","risk":"high","requester":"r","approver":"a",
            "state":"awaiting","justification":"j"}"#,
    )
    .unwrap_err();
    assert_eq!(e.path, "$.risk");

    let e = decode::<AdminApproval>(
        r#"{"id":"APR-1","action":"a","subsystem":"s","risk":1,"requester":"r","approver":"a",
            "state":"awaiting","justification":"j","extra":true}"#,
    )
    .unwrap_err();
    assert!(e.reason.contains("unknown key \"extra\""), "{e}");
}

#[test]
fn a_label_value_tuple_decodes_in_order_and_refuses_another_length() {
    // ForgeTransform.contractChecks (sdk/ts/src/products/forge.ts): `Array<[string, string]>`.
    let checks: Vec<(String, String)> = decode(r#"[["rows","12"],["nulls","0"]]"#).unwrap();
    assert_eq!(checks[1], ("nulls".to_string(), "0".to_string()));

    let e = decode::<Vec<(String, String)>>(r#"[["rows","12","x"]]"#).unwrap_err();
    assert_eq!(e.path, "$[0]");
    let e = decode::<Vec<(String, String)>>(r#"[["rows",12]]"#).unwrap_err();
    assert_eq!(e.path, "$[0][1]");
}

#[test]
fn a_missing_required_key_is_refused_by_name() {
    let e = decode::<AdminApproval>(
        r#"{"id":"APR-1","action":"a","subsystem":"s","risk":1,"requester":"r","approver":"a",
            "justification":"j"}"#,
    )
    .unwrap_err();
    assert_eq!(e.path, "$");
    assert!(e.reason.contains("missing key \"state\""), "{e}");
}

#[test]
fn a_number_outside_the_finite_range_is_refused() {
    let e = decode::<f64>("1e400").unwrap_err();
    assert_eq!(e.path, "$");
    let e = decode::<f64>("-1e400").unwrap_err();
    assert_eq!(e.path, "$");
    assert_eq!(decode::<f64>("1e300").unwrap(), 1e300);
}

#[test]
fn every_literal_union_field_publishes_its_allowed_values() {
    use midgard_client_core::consoles::admin::{
        ADMIN_APPROVAL_RISK_VALUES, ADMIN_APPROVAL_STATE_VALUES,
    };
    use midgard_client_core::consoles::midgard::MIDGARD_MARKING_LEVEL_VALUES;
    assert_eq!(
        ADMIN_APPROVAL_STATE_VALUES,
        &["awaiting", "approved", "denied", "executed"]
    );
    assert_eq!(ADMIN_APPROVAL_RISK_VALUES, &[1, 2, 3, 4, 5]);
    // The marking words the web contract allows; A8's level table must cover each.
    assert!(MIDGARD_MARKING_LEVEL_VALUES.contains(&"confidential"));
}

#[test]
fn every_marking_level_field_publishes_only_words_the_core_reads() {
    use midgard_client_core::consoles::common::POLICY_CONTEXT_CLEARANCE_VALUES;
    use midgard_client_core::consoles::forge::{
        FORGE_DATASET_MARKING_VALUES, FORGE_PIPELINE_MARKING_VALUES,
        FORGE_PROJECTION_MARKING_VALUES,
    };
    use midgard_client_core::consoles::heimdall::HEIMDALL_ENTITY_MARKING_VALUES;
    use midgard_client_core::consoles::midgard::{
        MIDGARD_GRAPH_NODE_MARKING_VALUES, MIDGARD_IDENTITY_CLEARANCE_VALUES,
        MIDGARD_MARKING_LEVEL_VALUES, MIDGARD_OBJECT_TYPE_MARKING_VALUES,
        MIDGARD_POLICY_RULE_MARKING_VALUES,
    };
    use midgard_client_core::marking::MarkingLevel;
    for values in [
        POLICY_CONTEXT_CLEARANCE_VALUES,
        FORGE_DATASET_MARKING_VALUES,
        FORGE_PIPELINE_MARKING_VALUES,
        FORGE_PROJECTION_MARKING_VALUES,
        HEIMDALL_ENTITY_MARKING_VALUES,
        MIDGARD_GRAPH_NODE_MARKING_VALUES,
        MIDGARD_IDENTITY_CLEARANCE_VALUES,
        MIDGARD_MARKING_LEVEL_VALUES,
        MIDGARD_OBJECT_TYPE_MARKING_VALUES,
        MIDGARD_POLICY_RULE_MARKING_VALUES,
    ] {
        for word in values {
            assert!(
                MarkingLevel::parse(word).is_some(),
                "{word:?} does not parse"
            );
        }
        // Every level the core knows is in the contract, and nothing else.
        let mut parsed: Vec<MarkingLevel> = values
            .iter()
            .filter_map(|w| MarkingLevel::parse(w))
            .collect();
        parsed.sort();
        assert_eq!(parsed, MarkingLevel::ALL);
    }
}

#[test]
fn heimdall_motion_decodes_strictly() {
    use midgard_client_core::consoles::motion::{HeimdallMotion, MotionSample};
    let m: HeimdallMotion = decode(
        r#"{"id":"E-1","provenance":"synthetic","samples":[{"lat":54.5,"lon":-3.74,"t":0},{"lat":54.6,"lon":-3.7,"t":10}]}"#,
    )
    .unwrap();
    assert_eq!(m.id, "E-1");
    assert_eq!(
        m.samples[1],
        MotionSample {
            t: 10,
            lat: 54.6,
            lon: -3.7
        }
    );
    for (bad, at) in [
        (r#"{"id":"E-1","provenance":"p","samples":[],"x":1}"#, "$"),
        (r#"{"id":"E-1","samples":[]}"#, "$"),
        (
            r#"{"id":"E-1","provenance":"p","samples":[{"lat":1,"lon":2}]}"#,
            "$.samples[0]",
        ),
        (
            r#"{"id":"E-1","provenance":"p","samples":[{"lat":91,"lon":2,"t":0}]}"#,
            "$.samples[0].lat",
        ),
        (
            r#"{"id":"E-1","provenance":"p","samples":[{"lat":1,"lon":-180.5,"t":0}]}"#,
            "$.samples[0].lon",
        ),
        (
            r#"{"id":"E-1","provenance":"p","samples":[{"lat":1,"lon":2,"t":-1}]}"#,
            "$.samples[0].t",
        ),
        (
            r#"{"id":"E-1","provenance":"p","samples":[{"lat":1,"lon":2,"t":10},{"lat":1,"lon":2,"t":10}]}"#,
            "$.samples[1].t",
        ),
    ] {
        let e = decode::<HeimdallMotion>(bad).unwrap_err();
        assert_eq!(e.path, at, "{bad}: {e}");
    }
}

#[test]
fn the_type_registry_knows_every_dataset_type_and_refuses_others() {
    use attest::json::parse;
    use midgard_client_core::consoles::check_item;
    let audit =
        parse(r#"{"id":"A","actor":"a","action":"b","resource":"c","ts":"d","pqSignature":"e"}"#)
            .unwrap();
    check_item("AuditRecord", &audit, "$").unwrap();
    assert!(check_item("AuditRecord", &parse("{}").unwrap(), "$").is_err());
    let e = check_item("NoSuchType", &audit, "$.x").unwrap_err();
    assert_eq!(e.path, "$.x");
    // Each type the committed fixtures name decodes through the registry.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../demo/fixtures/0.1.0-rc.3");
    let mut types = 0;
    for console in ["admin", "bifrost", "forge", "heimdall", "midgard", "mimir"] {
        let v =
            parse(&std::fs::read_to_string(dir.join(format!("{console}.json"))).unwrap()).unwrap();
        let attest::json::Json::Obj(colls) = v.get("collections").unwrap() else {
            panic!()
        };
        for (name, c) in colls {
            let ty = c.get("type").and_then(|t| t.as_str()).unwrap();
            for (i, r) in c
                .get("rows")
                .and_then(|r| r.as_array())
                .unwrap()
                .iter()
                .enumerate()
            {
                check_item(ty, r.get("item").unwrap(), &format!("{name}[{i}]"))
                    .unwrap_or_else(|e| panic!("{ty}: {e}"));
            }
            types += 1;
        }
    }
    assert_eq!(types, 53);
}
