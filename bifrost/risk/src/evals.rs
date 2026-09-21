//! The A.7 Eval suite (EV-A7-*). Each eval both passes on correct behaviour and
//! *catches* the corresponding violation. They score a parsed [`RiskAssessment`]
//! against A.7's guarantees: a critical CVE surfaces as a blocking concern, a clean
//! release is not over-flagged, and every top factor is backed by evidence. [FR-B-6]

use crate::assessment::{RiskAssessment, RiskBand};

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
    /// The eval id (e.g. `EV-A7-cve`).
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

/// EV-A7-cve — a release carrying a known critical CVE is scored high/critical and
/// raises at least one blocking concern. Catches an analyst that waves a critical
/// dependency through.
pub fn ev_a7_cve(a: &RiskAssessment) -> EvalResult {
    let ok =
        !a.blocking_concerns.is_empty() && matches!(a.band, RiskBand::High | RiskBand::Critical);
    EvalResult::of(
        "EV-A7-cve",
        ok,
        "critical CVE → high/critical band + a blocking concern",
    )
}

/// EV-A7-clean — a clean release (tests pass, no CVEs) is not over-flagged: no
/// blocking concerns and a low/elevated band. Catches an analyst that blocks
/// everything.
pub fn ev_a7_clean(a: &RiskAssessment) -> EvalResult {
    let ok = a.blocking_concerns.is_empty() && matches!(a.band, RiskBand::Low | RiskBand::Elevated);
    EvalResult::of(
        "EV-A7-clean",
        ok,
        "clean release → no blocking concerns, low/elevated band",
    )
}

/// EV-A7-evidence — every top factor cites non-empty evidence (A.7 must not
/// speculate). Catches an analyst that invents factors without grounding.
pub fn ev_a7_evidence(a: &RiskAssessment) -> EvalResult {
    let ok = a.top_factors.iter().all(|f| !f.evidence.trim().is_empty());
    EvalResult::of(
        "EV-A7-evidence",
        ok,
        "every top factor is backed by evidence",
    )
}
