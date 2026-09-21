//! The A.6 Eval suite (EV-A6-*). Each eval both passes on correct behaviour and
//! *catches* the corresponding violation — an eval that cannot fail proves nothing
//! (the same discipline as the A.1/A.9 suites). They score a [`TransformPreview`]:
//! a valid request yields an applicable transform, an invented column is flagged
//! (not applied), and the output is always a preview (never committed). [FR-F-7]

use crate::adapter::{TransformPreview, Validity};

/// A single eval score, mirroring the suite-wide harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalScore {
    /// Behaviour is correct.
    Pass,
    /// Behaviour is wrong.
    Fail,
}

/// One eval's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalResult {
    /// The eval id (e.g. `EV-A6-valid`).
    pub id: String,
    /// The score.
    pub score: EvalScore,
    /// A short explanation.
    pub detail: String,
}

impl EvalResult {
    /// Whether this eval passed.
    pub fn passed(&self) -> bool {
        self.score == EvalScore::Pass
    }
    fn of(id: &str, pass: bool, detail: &str) -> Self {
        Self {
            id: id.to_string(),
            score: if pass {
                EvalScore::Pass
            } else {
                EvalScore::Fail
            },
            detail: detail.to_string(),
        }
    }
}

/// EV-A6-valid — a satisfiable request produces a type-checking, applicable
/// transform (valid, uncommitted, with approvable stages). Catches an adapter that
/// fails to translate a valid library transform.
pub fn ev_a6_valid(preview: &TransformPreview) -> EvalResult {
    let ok = preview.is_valid() && !preview.is_committed() && preview.approved_stages().is_some();
    EvalResult::of(
        "EV-A6-valid",
        ok,
        "satisfiable request → valid, approvable, uncommitted preview",
    )
}

/// EV-A6-nocolumn — a response referencing a column not in the schema is flagged
/// `Invalid` and is NOT approvable. Catches an adapter that would pass an invented
/// column through as an applicable transform.
pub fn ev_a6_nocolumn(preview: &TransformPreview) -> EvalResult {
    let ok =
        matches!(preview.validity, Validity::Invalid { .. }) && preview.approved_stages().is_none();
    EvalResult::of(
        "EV-A6-nocolumn",
        ok,
        "invented column → Invalid, no approvable stages",
    )
}

/// EV-A6-preview — the adapter's output is always a preview: nothing is marked
/// committed, for any proposal. Catches an adapter that auto-applies a suggestion.
pub fn ev_a6_preview(preview: &TransformPreview) -> EvalResult {
    EvalResult::of(
        "EV-A6-preview",
        !preview.is_committed(),
        "output is a preview; the AI never commits a transform",
    )
}
