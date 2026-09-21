//! The pipeline IR — the **one dataset contract** the low-code builder and the
//! pro-code IDE both target. A [`Pipeline`] is an id, a source dataset branch, and
//! an ordered list of [`Transform`]s (the engine's IR). Because both authoring
//! surfaces lower to this exact structure, a pipeline authored visually and one
//! authored in code are indistinguishable downstream. [FR-F-1]

use forge_engine::Transform;

/// A named transform pipeline over a source dataset branch.
#[derive(Debug, Clone, PartialEq)]
pub struct Pipeline {
    /// Stable pipeline id (used in lineage and scheduling).
    pub id: String,
    /// The store branch whose head is the input dataset.
    pub source_branch: String,
    /// The ordered transform stages.
    pub stages: Vec<Transform>,
}

impl Pipeline {
    /// The **pro-code** constructor: author a pipeline directly from its stages.
    /// This yields exactly the same value the low-code [`crate::GraphBuilder`]
    /// produces for the same logical pipeline. [FR-F-1]
    pub fn code(
        id: impl Into<String>,
        source_branch: impl Into<String>,
        stages: Vec<Transform>,
    ) -> Self {
        Self {
            id: id.into(),
            source_branch: source_branch.into(),
            stages,
        }
    }
}
