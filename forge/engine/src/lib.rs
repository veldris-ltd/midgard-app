//! # forge-engine — the compute-backend seam
//!
//! Forge executes data transforms through a single trait, [`ComputeBackend`], so
//! the execution engine is a swappable backend rather than a hard-wired dependency.
//! Phase-1 ships an in-process, pure-Rust [`ReferenceBackend`] (the correctness
//! oracle) and a deterministic [`MockBackend`]; production backends — Polars,
//! DuckDB, Spark, Flink — implement the same trait and drop in without touching any
//! caller. This is the same seam pattern Mimir uses for its GPU model server.
//! [FR-F-1] [FR-F-5]
//!
//! ## The IR and its type system
//! [`ExecutionPlan`] is an ordered list of [`Transform`]s. [`Transform::apply_schema`]
//! and [`ExecutionPlan::output_schema`] infer and type-check the result schema
//! *without executing* — the pipeline's low-code builder and pro-code IDE both
//! lower to this IR and reuse this type-checker, so authoring surface and execution
//! can never disagree. [FR-F-1]
//!
//! ## Determinism
//! The reference backend has no threads, I/O, or clock: the same plan and input
//! always produce byte-identical output, which is what lets Forge guarantee
//! reproducible rebuilds. [AC-F-2] [NFR-PRV-1]

pub mod backend;
pub mod error;
pub mod mock;
pub mod partitioned;
pub mod plan;
pub mod reference;

pub use backend::ComputeBackend;
pub use error::EngineError;
pub use mock::MockBackend;
pub use partitioned::PartitionedBackend;
pub use plan::{
    AggFunc, AggregateSpec, BinOp, CmpOp, ExecutionPlan, Expr, JoinType, Predicate, SortKey,
    Transform, TransformLibrary, WindowFunc,
};
pub use reference::ReferenceBackend;
