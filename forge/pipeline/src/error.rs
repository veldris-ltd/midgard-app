//! Pipeline errors. A pipeline that does not type-check never executes, and a
//! missing source never silently yields an empty build.

use forge_engine::EngineError;

/// A pipeline compile- or run-time failure.
#[derive(Debug)]
pub enum PipelineError {
    /// The source branch has no committed head in the store.
    UnknownSourceBranch(String),
    /// The pipeline failed type inference/checking against its input schema.
    Type(EngineError),
    /// Executing the plan failed at runtime.
    Execution(EngineError),
    /// Reading the input or committing the output to the store failed.
    Store(String),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::UnknownSourceBranch(b) => {
                write!(f, "unknown source branch '{b}'")
            }
            PipelineError::Type(e) => write!(f, "pipeline type error: {e}"),
            PipelineError::Execution(e) => write!(f, "pipeline execution error: {e}"),
            PipelineError::Store(m) => write!(f, "store error: {m}"),
        }
    }
}

impl std::error::Error for PipelineError {}
