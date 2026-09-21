//! A deterministic **mock** [`ComputeBackend`] for tests and for exercising the
//! seam without the reference executor's semantics. Mirrors the deterministic mock
//! that stands in for Mimir's GPU model server: it lets callers assert the *wiring*
//! (plan lowered, backend invoked, output committed) independently of execution.

use forge_dataset_store::Dataset;

use crate::backend::ComputeBackend;
use crate::error::EngineError;
use crate::plan::ExecutionPlan;

/// How the mock responds to `execute`.
enum MockMode {
    /// Return the input unchanged (ignores the plan).
    Passthrough,
    /// Return a fixed dataset (ignores the input and plan).
    Fixed(Dataset),
}

/// A backend with fixed, deterministic behaviour.
pub struct MockBackend {
    mode: MockMode,
}

impl MockBackend {
    /// A backend that echoes its input dataset.
    pub fn passthrough() -> Self {
        Self {
            mode: MockMode::Passthrough,
        }
    }

    /// A backend that always returns `dataset`.
    pub fn returning(dataset: Dataset) -> Self {
        Self {
            mode: MockMode::Fixed(dataset),
        }
    }
}

impl ComputeBackend for MockBackend {
    fn name(&self) -> &str {
        "mock"
    }

    fn execute(&self, _plan: &ExecutionPlan, input: &Dataset) -> Result<Dataset, EngineError> {
        match &self.mode {
            MockMode::Passthrough => Ok(input.clone()),
            MockMode::Fixed(ds) => Ok(ds.clone()),
        }
    }
}
