//! The health gate: run a pipeline build, check its output against a health suite,
//! and **only promote (commit) if every expectation passes**. On failure the output
//! is discarded (never committed to the promotion branch) and the report explains
//! why — data-health is enforced on every build. [FR-F-6] [AC-F-3]

use forge_dataset_store::{DatasetStore, DatasetVersion, Schema};
use forge_engine::ComputeBackend;
use forge_pipeline::{build, commit_build, Pipeline, PipelineError};
use kms::TrustRoot;
use pqc_agility::Rng;

use crate::suite::{HealthReport, HealthSuite};

/// The outcome of a health-gated build.
#[derive(Debug, Clone)]
pub struct GateOutcome {
    /// The health report for the build's output.
    pub report: HealthReport,
    /// The promoted version, or `None` if promotion was **blocked** by a failing
    /// expectation. [AC-F-3]
    pub promoted: Option<DatasetVersion>,
    /// The type-checked output schema (available whether or not it was promoted).
    pub output_schema: Schema,
}

impl GateOutcome {
    /// Whether the build was promoted.
    pub fn was_promoted(&self) -> bool {
        self.promoted.is_some()
    }
}

/// Build `pipeline`, evaluate `suite` on the (uncommitted) output, and promote to
/// `out_branch` only if the suite passes. A failing suite blocks promotion — the
/// output is not committed and `promoted` is `None`. Compile/exec errors still fail
/// closed. [FR-F-6] [AC-F-3]
pub fn run_gated<R: TrustRoot, B: ComputeBackend>(
    pipeline: &Pipeline,
    store: &mut DatasetStore<R>,
    backend: &B,
    suite: &HealthSuite,
    out_branch: &str,
    rng: &mut dyn Rng,
) -> Result<GateOutcome, PipelineError> {
    let built = build(pipeline, store, backend)?;
    let report = suite.check(&built.output);
    let output_schema = built.output_schema.clone();

    if report.passed {
        let version = commit_build(&built, store, out_branch, rng)?;
        Ok(GateOutcome {
            report,
            promoted: Some(version),
            output_schema,
        })
    } else {
        // Promotion blocked: nothing is written to `out_branch`.
        Ok(GateOutcome {
            report,
            promoted: None,
            output_schema,
        })
    }
}
