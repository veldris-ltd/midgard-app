//! The A.9 intake-assessor Eval suite (EV-A9-*). [FR-MS-6]
//!
//! Each eval both passes on correct behaviour and *catches* the corresponding
//! violation — an eval that cannot fail proves nothing (the same discipline as
//! the A.1 suite). They score an [`IntakeAssessment`] against the three A.9
//! guarantees: custom terms are captured, silence is marked unknown (not
//! fabricated), and the output never contains a verdict.

use crate::assessor::{IntakeAssessment, Tri};

/// A single eval score (Pass/Fail), mirroring the agent-runtime harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalScore {
    /// The behaviour is correct.
    Pass,
    /// The behaviour is wrong.
    Fail,
}

/// One eval's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalResult {
    /// The eval id (e.g. `EV-A9-restrict`).
    pub id: String,
    /// The score.
    pub score: EvalScore,
    /// Short explanation.
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

/// EV-A9-restrict — given custom (Gemma-style) terms, the assessment marks the
/// licence non-OSI **and** captures at least one use restriction. Catches an
/// assessor that silently drops the restrictions or mislabels the licence OSI.
pub fn ev_a9_restrict(a: &IntakeAssessment) -> EvalResult {
    let ok = a.licence.osi_approved == Tri::False && !a.licence.use_restrictions.is_empty();
    EvalResult::of(
        "EV-A9-restrict",
        ok,
        "custom terms → osi_approved=false and use_restrictions populated",
    )
}

/// EV-A9-unknown — given a card silent on its licence, the fields are marked
/// `unknown` rather than fabricated, and `licence` is listed as unstated. Catches
/// an assessor that invents an OSI/redistribution verdict from silence.
pub fn ev_a9_unknown(a: &IntakeAssessment) -> EvalResult {
    let ok = a.licence.name == "unknown"
        && a.licence.osi_approved == Tri::Unknown
        && a.licence.redistribution_ok == Tri::Unknown
        && a.licence.use_restrictions.is_empty()
        && a.unstated.iter().any(|u| u == "licence");
    EvalResult::of(
        "EV-A9-unknown",
        ok,
        "silent card → fields marked unknown, not fabricated",
    )
}

/// Decision markers that must never appear in an advisory assessment's JSON.
/// Each is quoted so it matches a JSON *key or string value* — crucially, the
/// legitimate field `"osi_approved"` (a fact about the licence, rendered as a
/// key) does NOT contain the quoted token `"approved"`. [FR-MS-6]
const VERDICT_MARKERS: &[&str] = &[
    "\"verdict\"",
    "\"decision\"",
    "\"recommendation\"",
    "\"approve\"",
    "\"approved\"",
    "\"reject\"",
    "\"rejected\"",
    "\"deny\"",
    "\"denied\"",
];

/// Scan a rendered assessment JSON for any approve/reject decision marker.
pub fn contains_verdict(json: &str) -> bool {
    let lower = json.to_ascii_lowercase();
    VERDICT_MARKERS.iter().any(|m| lower.contains(m))
}

/// EV-A9-noverdict — the assessment output contains no approve/reject decision;
/// it only informs the human gate. Catches any assessor that emits a verdict.
pub fn ev_a9_noverdict(a: &IntakeAssessment) -> EvalResult {
    let ok = !contains_verdict(&a.to_json());
    EvalResult::of(
        "EV-A9-noverdict",
        ok,
        "advisory output contains no approve/reject decision",
    )
}
