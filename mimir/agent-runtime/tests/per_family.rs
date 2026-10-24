//! The self-* prompt-level Evals run **per model family** (Mistral / Nemotron / Gemma / Granite),
//! so each prompt is certified against every model it may be routed to (FR-MS-7).
//! The reference classifiers are family-invariant stand-ins; the real per-family
//! models plug in behind the same seams.

use agent_runtime::{
    per_family, run_ev_a3, run_ev_a4, run_ev_a5, FeedbackDistiller, SelfDefending, SelfHealing,
    MODEL_FAMILIES,
};

#[test]
fn a3_a4_a5_evals_pass_for_every_model_family() {
    let sd = SelfDefending::new();
    let sup = SelfHealing::new();
    let dist = FeedbackDistiller::new();

    // The reference classifiers are family-invariant, so they ignore the family
    // argument; a model-backed suite dispatches on it to the family's served model.
    let suites = [
        ("A.3", per_family(|_family| run_ev_a3(&sd))),
        ("A.4", per_family(|_family| run_ev_a4(&sup))),
        ("A.5", per_family(|_family| run_ev_a5(&dist))),
    ];

    let mut certified = 0;
    for (prompt, per_fam) in &suites {
        assert_eq!(per_fam.len(), MODEL_FAMILIES.len(), "one run per family");
        for (family, results) in per_fam {
            for r in results {
                assert!(
                    r.passed(),
                    "{prompt} {} failed for family {family}: {}",
                    r.id,
                    r.detail
                );
                certified += 1;
            }
        }
    }
    // 3 suites × 4 families × 4 evals = 48 certifications.
    assert_eq!(certified, 48);
}
