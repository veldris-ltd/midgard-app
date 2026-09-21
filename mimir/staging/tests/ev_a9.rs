//! The A.9 intake-assessor Eval suite (EV-A9-*), output level. Each eval passes on
//! correct behaviour and *catches* the corresponding violation. [FR-MS-6]

use staging::{
    contains_verdict, ev_a9_noverdict, ev_a9_restrict, ev_a9_unknown, AssessedLicence,
    IntakeAssessment, ModelIntakeAssessor, Tri,
};

/// A Gemma-style card: custom, non-OSI terms with an explicit prohibited-uses
/// section.
const GEMMA_CARD: &str = "\
# Gemma-2-9B (fine-tuned)

Licensed under the Google Gemma Terms of Use.

## Intended use
General assistant tasks.

## Prohibited Uses:
- You may not use the model to generate disinformation.
- You may not use the model for unlawful surveillance.

## Provenance
Fine-tuned from google/gemma-2-9b.

## Safety
Evaluated for toxicity and bias.
";

/// A card that says nothing about its licence.
const SILENT_CARD: &str = "\
# FastModel

A very fast and capable model. Enjoy using it!
";

const META: &str = "source: huggingface; vendor signature present; commit abc123";

#[test]
fn ev_a9_restrict_passes_and_catches() {
    // Real assessment of custom terms: non-OSI, restrictions captured → pass.
    let a = ModelIntakeAssessor::new().assess(GEMMA_CARD, META);
    assert_eq!(a.licence.osi_approved, Tri::False);
    assert!(!a.licence.use_restrictions.is_empty());
    assert!(ev_a9_restrict(&a).passed());

    // An assessor that mislabels the custom licence OSI, or drops the
    // restrictions, must be caught.
    let mislabeled_osi = IntakeAssessment {
        licence: AssessedLicence {
            osi_approved: Tri::True,
            ..a.licence.clone()
        },
        ..a.clone()
    };
    assert!(!ev_a9_restrict(&mislabeled_osi).passed());

    let dropped_restrictions = IntakeAssessment {
        licence: AssessedLicence {
            use_restrictions: vec![],
            ..a.licence.clone()
        },
        ..a.clone()
    };
    assert!(!ev_a9_restrict(&dropped_restrictions).passed());
}

#[test]
fn ev_a9_unknown_passes_and_catches() {
    // A silent card: every licence fact unknown, not fabricated → pass.
    let a = ModelIntakeAssessor::new().assess(SILENT_CARD, META);
    assert_eq!(a.licence.name, "unknown");
    assert_eq!(a.licence.osi_approved, Tri::Unknown);
    assert_eq!(a.licence.redistribution_ok, Tri::Unknown);
    assert!(a.licence.use_restrictions.is_empty());
    assert!(a.unstated.iter().any(|u| u == "licence"));
    assert!(ev_a9_unknown(&a).passed());

    // An assessor that fabricates a verdict from silence must be caught.
    let fabricated = IntakeAssessment {
        licence: AssessedLicence {
            name: "MIT".into(),
            osi_approved: Tri::True,
            redistribution_ok: Tri::True,
            use_restrictions: vec![],
        },
        ..a.clone()
    };
    assert!(!ev_a9_unknown(&fabricated).passed());
}

#[test]
fn ev_a9_noverdict_passes_and_catches() {
    // The real advisory output carries no approve/reject decision → pass.
    let a = ModelIntakeAssessor::new().assess(GEMMA_CARD, META);
    assert!(ev_a9_noverdict(&a).passed());
    assert!(!contains_verdict(&a.to_json()));

    // The scanner must CATCH a verdict-bearing output …
    assert!(contains_verdict("{\"verdict\": \"approve\"}"));
    assert!(contains_verdict("{\"decision\": \"reject\", \"x\": 1}"));
    assert!(contains_verdict("{\"recommendation\": \"approved\"}"));

    // … but must NOT false-trip on the legitimate `osi_approved` licence fact,
    // which is a property of the licence, not a decision by the assessor.
    assert!(!contains_verdict("{\"osi_approved\": true}"));
    assert!(a.to_json().contains("\"osi_approved\""));
}
