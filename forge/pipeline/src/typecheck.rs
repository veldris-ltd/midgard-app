//! The pipeline type system: infer and type-check a pipeline's output schema
//! against its input schema, before any data is touched. Delegates to the engine's
//! single [`ExecutionPlan::output_schema`] inference so "does it compile" here and
//! "what it computes" in the executor are the same judgement. [FR-F-1]

use forge_dataset_store::Schema;
use forge_engine::ExecutionPlan;

use crate::error::PipelineError;
use crate::pipeline::Pipeline;

/// Type-check `pipeline` against `input`, returning the inferred output schema or a
/// [`PipelineError::Type`]. Pure — no execution.
pub fn typecheck(pipeline: &Pipeline, input: &Schema) -> Result<Schema, PipelineError> {
    let plan = ExecutionPlan::new(pipeline.stages.clone());
    plan.output_schema(input).map_err(PipelineError::Type)
}
