//! Codegen: lower a (typically optimised) [`Pipeline`] to an engine
//! [`ExecutionPlan`]. The pipeline IR and the engine IR share the same [`Transform`]
//! vocabulary, so lowering is a direct handoff today — but keeping it a distinct
//! step is the seam where a future backend-specific lowering (Polars/Spark plan
//! objects) would live. [FR-F-1]

use forge_engine::ExecutionPlan;

use crate::pipeline::Pipeline;

/// Lower a pipeline to an executable plan for the compute backend.
pub fn codegen(pipeline: &Pipeline) -> ExecutionPlan {
    ExecutionPlan::new(pipeline.stages.clone())
}
