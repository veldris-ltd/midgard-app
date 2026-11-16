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
