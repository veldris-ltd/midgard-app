//! AC-F-5 — Given an AI-suggested transform, when proposed, then it is shown as a
//! preview diff and is not committed without approval. [FR-F-7] [NFR-AI-1]
//! Plus the EV-A6-* suite (valid / nocolumn / preview).

use forge_ai_assist::{
    ev_a6_nocolumn, ev_a6_preview, ev_a6_valid, suggest_transform, MockSuggester,
    UnavailableSuggester, Validity,
};
use forge_dataset_store::{Column, ColumnType, Schema};

fn schema() -> Schema {
    Schema::new(vec![
        Column::new("region", ColumnType::Text),
        Column::new("units", ColumnType::Int),
        Column::new("price", ColumnType::Float),
    ])
}

// A well-formed A.6 response: keep units >= 100, then derive revenue = units? No —
// revenue = price * price (just to exercise derive), then select region+revenue.
const VALID_RESPONSE: &str = r#"
{
  "transform": [
    {"op": "filter", "args": {"column": "units", "cmp": "ge", "value": 100}, "output_columns": []},
    {"op": "derive", "args": {"name": "revenue", "expr": {"binop": "mul", "left": {"col": "price"}, "right": {"col": "price"}}, "ty": "float"}, "output_columns": ["revenue"]},
    {"op": "select", "args": {"columns": ["region", "revenue"]}, "output_columns": ["region", "revenue"]}
  ],
  "explanation": "Filter to high-volume regions and compute a revenue proxy.",
  "unsupported": null
}
"#;

// A response that invents a column ('quantity' does not exist).
const NOCOLUMN_RESPONSE: &str = r#"
{
  "transform": [
    {"op": "filter", "args": {"column": "quantity", "cmp": "ge", "value": 100}, "output_columns": []}
  ],
  "explanation": "Filter on quantity.",
  "unsupported": null
}
"#;

// A response where the model declines.
const UNSUPPORTED_RESPONSE: &str = r#"
{
  "transform": [],
  "explanation": "",
  "unsupported": "no pivot operation exists in the transform library"
}
"#;

#[test]
fn ac_f_5_valid_suggestion_is_a_preview_and_is_not_committed() {
    let suggester = MockSuggester::new(VALID_RESPONSE);
    let preview = suggest_transform(&schema(), "top regions by revenue", &suggester).unwrap();

    // It is a preview diff: input → output schema is available, and it is NOT
    // committed. The AI never mutates the dataset. [AC-F-5] [NFR-AI-1]
    assert!(preview.is_valid());
    assert!(!preview.is_committed());
    let out = preview
        .output_schema()
        .expect("valid preview has an output schema");
    // The diff's "after" side: region + revenue.
    assert_eq!(out.width(), 2);
    assert!(out.column("revenue").is_some());
    assert!(out.column("units").is_none()); // dropped by the select

    // Applying requires an explicit human step; the adapter never runs it.
    assert!(preview.approved_stages().is_some());
    assert_eq!(preview.approved_stages().unwrap().len(), 3);
}

#[test]
fn ev_a6_valid_passes_for_a_satisfiable_request() {
    let suggester = MockSuggester::new(VALID_RESPONSE);
    let preview = suggest_transform(&schema(), "top regions", &suggester).unwrap();
    let r = ev_a6_valid(&preview);
    assert!(r.passed(), "{}: {}", r.id, r.detail);
    // And it is still, always, a preview.
    assert!(ev_a6_preview(&preview).passed());
}

#[test]
fn ev_a6_nocolumn_flags_an_invented_column_and_blocks_application() {
    let suggester = MockSuggester::new(NOCOLUMN_RESPONSE);
    let preview = suggest_transform(&schema(), "filter on quantity", &suggester).unwrap();

    // The invented column is caught by the engine type system → Invalid, and there
    // are NO approvable stages (it cannot be applied).
    assert!(matches!(preview.validity, Validity::Invalid { .. }));
    assert!(preview.approved_stages().is_none());
    assert!(!preview.is_committed());
    assert!(ev_a6_nocolumn(&preview).passed());
    // Even an invalid proposal is still a (non-committed) preview.
    assert!(ev_a6_preview(&preview).passed());
}

#[test]
fn unsupported_request_is_reported_not_fabricated() {
    let suggester = MockSuggester::new(UNSUPPORTED_RESPONSE);
    let preview = suggest_transform(&schema(), "pivot the table", &suggester).unwrap();
    assert!(matches!(preview.validity, Validity::Unsupported { .. }));
    assert!(preview.approved_stages().is_none());
    assert!(!preview.is_committed());
}

#[test]
fn inference_failure_fails_closed() {
    // With Mimir unreachable, the adapter errors rather than inventing a transform.
    let preview = suggest_transform(&schema(), "anything", &UnavailableSuggester);
    assert!(preview.is_err());
}

#[test]
fn malformed_json_fails_closed() {
    let suggester = MockSuggester::new("{ not json ");
    assert!(suggest_transform(&schema(), "x", &suggester).is_err());
}
