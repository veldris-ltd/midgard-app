//! # forge-pipeline — the pipeline engine
//!
//! One IR, two authoring surfaces. A [`Pipeline`] is the single dataset-transform
//! contract that both the low-code [`GraphBuilder`] and the pro-code
//! [`Pipeline::code`] path produce; they are byte-identical for the same logical
//! pipeline, so visual and code authoring interoperate. [FR-F-1]
//!
//! ## The compile → run path
//! [`typecheck`] infers the output schema and rejects type errors before any data
//! is read; [`optimise`] pushes filters down (result-preserving); [`codegen`]
//! lowers to an engine [`forge_engine::ExecutionPlan`]; [`run`] executes it on a
//! [`forge_engine::ComputeBackend`] and commits a sealed, lineage-bearing output
//! version. With the reference backend the whole path is deterministic, so a
//! pipeline built twice from the same inputs is content-identical. [AC-F-2]
//!
//! ## Scheduling
//! [`Scheduler`] manages batch and streaming triggers deterministically and
//! clock-free; runs reuse [`run`]. [FR-F-5]

pub mod builder;
pub mod codegen;
pub mod error;
pub mod optimise;
pub mod pipeline;
pub mod run;
pub mod scheduler;
pub mod typecheck;

pub use builder::GraphBuilder;
pub use codegen::codegen;
pub use error::PipelineError;
pub use optimise::optimise;
pub use pipeline::Pipeline;
pub use run::{build, commit_build, run, BuildOutcome, BuiltDataset};
pub use scheduler::{Schedule, Scheduler, Trigger};
pub use typecheck::typecheck;
