//! AC-H-3 — Given a decision point, when options are requested, then Mimir returns
//! ranked options each citing the evidence used. Plus the full A.8 Eval suite
//! (EV-A8-*): options are cited, grounded in the picture, distinctly ranked, balanced
//! (risks surfaced, non-advocating), insufficiency is handled explicitly, and the
//! result is strictly advisory. [FR-H-4] [NFR-AI-1]
//!
//! The provider is a deterministic mock standing in for the Mimir bridge — the same
//! seam-with-mock pattern as the A.6/A.7 suites.

use std::collections::{BTreeMap, BTreeSet};

use heimdall_decision_support::{
    ev_a8_advisory, ev_a8_cited, ev_a8_grounded, ev_a8_insuff, ev_a8_neutral, ev_a8_ranked,
    request_options, DecisionError, MockOptionProvider, UnavailableOptionProvider,
};
use heimdall_picture::{
    Classification, ClassificationSource, EntityId, LayerId, Picture, Track, TrackedEntity,
};

/// A two-entity picture: A123 (from feed `radar`) and S9 (from feed `sonar`).
fn sample_picture() -> Picture {
    let mut p = Picture::new();
    p.upsert_entity(entity("A123", "air-track", "air", "radar", "fast-mover"));
    p.upsert_entity(entity("S9", "sea-track", "sea", "sonar", "surface-contact"));
    p
}

fn entity(id: &str, kind: &str, layer: &str, feed: &str, label: &str) -> TrackedEntity {
    let mut sources = BTreeSet::new();
    sources.insert(feed.to_string());
    TrackedEntity {
        id: EntityId(id.into()),
        entity_key: id.into(),
        kind: kind.into(),
        layer: LayerId(layer.into()),
        sources,
        track: Track {
            first_seen: 100,
            last_seen: 105,
            observation_count: 2,
        },
        classification: Some(Classification {
            label: label.into(),
            source: ClassificationSource::Rules,
            rationale: "rule".into(),
        }),
        latest_attrs: BTreeMap::new(),
    }
}

const GOOD: &str = r#"{
  "options": [
    {"rank": 1, "title": "Cue radar on A123", "summary": "Task the radar for a closer look at the fast mover.",
     "citations": ["entity:A123", "feed:radar"], "expected_effect": "tighter track", "risks": ["sensor tasking cost"]},
    {"rank": 2, "title": "Hold and watch S9", "summary": "Keep the maritime contact under passive watch.",
     "citations": ["entity:S9", "feed:sonar"], "expected_effect": "no escalation", "risks": ["late reaction"]}
  ],
  "unsupported": null
}"#;

#[test]
fn ac_h_3_options_are_ranked_and_cite_evidence() {
    let picture = sample_picture();
    let provider = MockOptionProvider::new(GOOD);

    let advisory = request_options(
        &picture,
        "How should we handle the fast air track?",
        &provider,
    )
    .expect("options returned");

    // Ranked, and each option cites specific evidence.
    assert_eq!(advisory.option_set.options.len(), 2);
    assert_eq!(advisory.option_set.options[0].rank, 1);
    assert_eq!(advisory.option_set.options[1].rank, 2);
    for opt in &advisory.option_set.options {
        assert!(!opt.citations.is_empty(), "every option cites evidence");
    }

    // All citations resolve to the picture (grounded), and the result is advisory.
    assert!(advisory.all_grounded());
    assert!(advisory.is_advisory());

    // The full EV-A8 suite passes on well-formed output: options are present with a
    // reason absent (insuff), and each option surfaces risks + a summary (neutral).
    assert!(ev_a8_cited(&advisory).passed());
    assert!(ev_a8_grounded(&advisory).passed());
    assert!(ev_a8_ranked(&advisory).passed());
    assert!(ev_a8_insuff(&advisory).passed());
    assert!(ev_a8_neutral(&advisory).passed());
    assert!(ev_a8_advisory(&advisory).passed());
}

#[test]
fn ev_a8_grounded_catches_invented_evidence() {
    // An option citing an entity that is NOT in the picture (a hallucinated
    // reference) must be reported ungrounded — EV-A8-grounded catches it.
    let picture = sample_picture();
    let invented = r#"{"options":[{"rank":1,"title":"Strike GHOST","summary":"act on a phantom",
        "citations":["entity:GHOST"],"expected_effect":"","risks":[]}],"unsupported":null}"#;
    let advisory = request_options(&picture, "decide", &MockOptionProvider::new(invented)).unwrap();

    assert!(!advisory.all_grounded());
    assert!(!ev_a8_grounded(&advisory).passed());
    // It still *cited* something, so the cited-eval passes — the two evals catch
    // different failure modes.
    assert!(ev_a8_cited(&advisory).passed());
    let g = advisory.grounding_for(1).unwrap();
    assert_eq!(g.unresolved.len(), 1);
    assert_eq!(g.unresolved[0].id, "GHOST");
}

#[test]
fn ev_a8_cited_catches_uncited_option() {
    let picture = sample_picture();
    let uncited = r#"{"options":[{"rank":1,"title":"Do something","summary":"no basis",
        "citations":[],"expected_effect":"","risks":[]}],"unsupported":null}"#;
    let advisory = request_options(&picture, "decide", &MockOptionProvider::new(uncited)).unwrap();

    assert!(!ev_a8_cited(&advisory).passed());
}

#[test]
fn adapter_fails_closed_when_mimir_unavailable() {
    // With no reachable model the adapter returns an error rather than fabricating
    // options — decision support is simply unavailable, the rest of Heimdall runs.
    let picture = sample_picture();
    let err = request_options(&picture, "decide", &UnavailableOptionProvider).unwrap_err();
    assert!(matches!(err, DecisionError::Infer(_)));
}

#[test]
fn unsupported_when_picture_cannot_support_a_decision() {
    let picture = sample_picture();
    let none = r#"{"options":[],"unsupported":"no actionable contacts in view"}"#;
    let advisory = request_options(&picture, "decide", &MockOptionProvider::new(none)).unwrap();
    assert!(advisory.option_set.options.is_empty());
    assert_eq!(
        advisory.option_set.unsupported.as_deref(),
        Some("no actionable contacts in view")
    );
    // A *correct* insufficiency response (empty options + a reason) is a first-class
    // legal shape: EV-A8-insuff passes, and the populated-path evals are vacuously true
    // rather than falsely failing. The whole suite is green.
    assert!(ev_a8_insuff(&advisory).passed());
    assert!(ev_a8_cited(&advisory).passed());
    assert!(ev_a8_grounded(&advisory).passed());
    assert!(ev_a8_ranked(&advisory).passed());
    assert!(ev_a8_neutral(&advisory).passed());
    assert!(ev_a8_advisory(&advisory).passed());
    // The strict adapter predicate still reports "nothing to stand on" for the empty
    // set — that is deliberately distinct from the eval's vacuous pass.
    assert!(!advisory.all_grounded());
}

#[test]
fn ev_a8_insuff_catches_silent_empty_answer() {
    // Empty options with NO reason is a silent non-answer — EV-A8-insuff must catch it
    // even though the populated-path evals are vacuously true.
    let picture = sample_picture();
    let silent = r#"{"options":[],"unsupported":null}"#;
    let advisory = request_options(&picture, "decide", &MockOptionProvider::new(silent)).unwrap();
    assert!(!ev_a8_insuff(&advisory).passed());
    // The others don't fail here — insuff is the only gate for this shape.
    assert!(ev_a8_ranked(&advisory).passed());
    assert!(ev_a8_cited(&advisory).passed());
}

#[test]
fn ev_a8_insuff_catches_contradictory_response() {
    // Offering options *and* declaring the decision unsupported is contradictory.
    let picture = sample_picture();
    let both = r#"{"options":[{"rank":1,"title":"Act","summary":"do it",
        "citations":["entity:A123"],"expected_effect":"x","risks":["cost"]}],
        "unsupported":"but also unsupported"}"#;
    let advisory = request_options(&picture, "decide", &MockOptionProvider::new(both)).unwrap();
    assert!(!ev_a8_insuff(&advisory).passed());
}

#[test]
fn ev_a8_neutral_catches_risk_suppressed_option() {
    // An option presented with no risks is a railroaded, consequence-free framing —
    // EV-A8-neutral catches it, while cited/grounded/ranked still pass.
    let picture = sample_picture();
    let riskfree = r#"{"options":[{"rank":1,"title":"Strike A123","summary":"engage now",
        "citations":["entity:A123"],"expected_effect":"neutralised","risks":[]}],
        "unsupported":null}"#;
    let advisory = request_options(&picture, "decide", &MockOptionProvider::new(riskfree)).unwrap();
    assert!(!ev_a8_neutral(&advisory).passed());
    assert!(ev_a8_cited(&advisory).passed());
    assert!(ev_a8_grounded(&advisory).passed());
    assert!(ev_a8_ranked(&advisory).passed());
}
