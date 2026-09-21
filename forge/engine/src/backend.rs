//! The compute-backend seam. `execute` runs a type-checked [`ExecutionPlan`] over
//! an input [`Dataset`] and returns the output dataset. The reference backend runs
//! in-process and pure-Rust; production backends (Polars, DuckDB, Spark, Flink)
//! implement the *same trait* and are selected without changing any caller — the
//! same seam pattern as Mimir's model server. [FR-F-1] [FR-F-5]

use forge_dataset_store::Dataset;

use crate::error::EngineError;
use crate::plan::ExecutionPlan;

/// A compute backend that executes transform plans over datasets.
pub trait ComputeBackend {
    /// A short backend identifier (recorded in build lineage/observability).
    fn name(&self) -> &str;

    /// Execute `plan` over `input`, producing the output dataset. Must be
    /// deterministic: the same plan and input always yield byte-identical output,
    /// which is what makes rebuilds reproducible. [AC-F-2]
    fn execute(&self, plan: &ExecutionPlan, input: &Dataset) -> Result<Dataset, EngineError>;
}
