//! Build runs. A build has two stages so a health gate can sit between them:
//! [`build`] compiles and executes a pipeline into an **uncommitted** output
//! dataset; [`commit_build`] promotes that output into the store as a sealed,
//! lineage-bearing version. [`run`] is the un-gated convenience that does both.
//! The health gate (Phase 4.c) calls `build`, checks expectations, and only then
//! `commit_build`s — blocking promotion on failure. [FR-F-1] [FR-F-6] [AC-F-2]

use forge_dataset_store::{ContentId, Dataset, DatasetStore, DatasetVersion, Lineage, Schema};
use forge_engine::ComputeBackend;
use kms::TrustRoot;
use pqc_agility::Rng;

use crate::codegen::codegen;
use crate::error::PipelineError;
use crate::optimise::optimise;
use crate::pipeline::Pipeline;
use crate::typecheck::typecheck;

/// A compiled-and-executed but **not yet committed** build output. Holding this
/// without promoting it is exactly what lets the health gate block a bad build.
#[derive(Debug, Clone)]
pub struct BuiltDataset {
    /// The id of the pipeline that produced this output.
    pub pipeline_id: String,
    /// The computed output dataset (uncommitted).
    pub output: Dataset,
    /// The type-checked output schema.
    pub output_schema: Schema,
    /// The content id of the input this build consumed.
    pub input_id: ContentId,
    /// The compute backend that executed the plan.
    pub backend: String,
    /// The optimised pipeline actually executed.
    pub optimised: Pipeline,
}

/// The result of a committed (promoted) build.
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    /// The type-checked output schema.
    pub output_schema: Schema,
    /// The committed output version (content-addressed, sealed).
    pub version: DatasetVersion,
    /// The content id of the input this build consumed.
    pub input_id: ContentId,
    /// The compute backend that executed the plan.
    pub backend: String,
    /// The optimised pipeline actually executed.
    pub optimised: Pipeline,
}

/// Compile (type-check → optimise → codegen) and execute a pipeline over its source
/// dataset, returning the **uncommitted** output. Fails closed on a missing source,
/// a type error, or an execution error. Borrows the store immutably — nothing is
/// written. [FR-F-1]
pub fn build<R: TrustRoot, B: ComputeBackend>(
    pipeline: &Pipeline,
    store: &DatasetStore<R>,
    backend: &B,
) -> Result<BuiltDataset, PipelineError> {
    let input_id = store
        .branch_head(&pipeline.source_branch)
        .ok_or_else(|| PipelineError::UnknownSourceBranch(pipeline.source_branch.clone()))?;
    let input = store
        .checkout(&input_id)
        .map_err(|e| PipelineError::Store(e.to_string()))?;

    let output_schema = typecheck(pipeline, input.schema())?;
    let optimised = optimise(pipeline);
    let plan = codegen(&optimised);
    let output = backend
        .execute(&plan, &input)
        .map_err(PipelineError::Execution)?;

    Ok(BuiltDataset {
        pipeline_id: pipeline.id.clone(),
        output,
        output_schema,
        input_id,
        backend: backend.name().to_string(),
        optimised,
    })
}

/// Promote a built output into the store on `out_branch`, sealing it and recording
/// transform lineage that ties it to its input. This is the "promotion" step a
/// health gate guards. [FR-F-3] [NFR-PQ-2]
pub fn commit_build<R: TrustRoot>(
    built: &BuiltDataset,
    store: &mut DatasetStore<R>,
    out_branch: &str,
    rng: &mut dyn Rng,
) -> Result<DatasetVersion, PipelineError> {
    let lineage = Lineage::transform(
        format!("pipeline:{}", built.pipeline_id),
        "transform:pipeline",
        vec![built.input_id],
    );
    store
        .commit(&built.output, out_branch, lineage, rng)
        .map_err(|e| PipelineError::Store(e.to_string()))
}

/// Run a pipeline build end-to-end (un-gated): [`build`] then [`commit_build`].
/// [FR-F-1] [AC-F-2]
pub fn run<R: TrustRoot, B: ComputeBackend>(
    pipeline: &Pipeline,
    store: &mut DatasetStore<R>,
    backend: &B,
    out_branch: &str,
    rng: &mut dyn Rng,
) -> Result<BuildOutcome, PipelineError> {
    let built = build(pipeline, store, backend)?;
    let version = commit_build(&built, store, out_branch, rng)?;
    Ok(BuildOutcome {
        output_schema: built.output_schema,
        version,
        input_id: built.input_id,
        backend: built.backend,
        optimised: built.optimised,
    })
}
