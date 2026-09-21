//! The release-intelligence entry point: assemble A.7, call the Mimir advisor seam,
//! and parse the advisory [`RiskAssessment`]. This produces the *score*; the
//! deterministic [`crate::gate`] decides go/no-go. [FR-B-6]

use crate::advisor::RiskAdvisor;
use crate::assessment::RiskAssessment;
use crate::context::ReleaseContext;
use crate::error::RiskError;
use crate::prompt::assemble;

/// Ask Mimir (via `advisor`) for an advisory risk assessment of `context`. Fails
/// closed if inference fails or the response is malformed — the caller can still
/// run the deterministic gate on hard rules alone. [FR-B-6] [AC-B-5]
pub fn assess<A: RiskAdvisor>(
    context: &ReleaseContext,
    advisor: &A,
) -> Result<RiskAssessment, RiskError> {
    let prompt = assemble(context);
    let raw = advisor.assess(&prompt)?;
    RiskAssessment::parse(&raw)
}
