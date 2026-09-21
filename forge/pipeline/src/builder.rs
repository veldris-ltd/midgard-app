//! The **low-code** builder: assemble a pipeline node-by-node, exactly as the
//! visual studio would. Its [`GraphBuilder::build`] output is a [`Pipeline`] —
//! byte-identical to what the pro-code [`Pipeline::code`] path produces — so the
//! two surfaces are interoperable over one contract. [FR-F-1]

use forge_dataset_store::ColumnType;
use forge_engine::{Expr, Predicate, Transform};

use crate::pipeline::Pipeline;

/// A fluent, append-only pipeline builder (each call adds one node/stage).
#[derive(Debug, Clone)]
pub struct GraphBuilder {
    id: String,
    source_branch: String,
    stages: Vec<Transform>,
}

impl GraphBuilder {
    /// Start a graph over a source dataset branch.
    pub fn new(id: impl Into<String>, source_branch: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            source_branch: source_branch.into(),
            stages: Vec::new(),
        }
    }

    /// Append an arbitrary transform node.
    ///
    /// Named `stage` rather than `add` so it cannot be mistaken for
    /// `std::ops::Add::add` — this consumes and returns `Self` as a builder step,
    /// which is not what an `Add` impl means.
    pub fn stage(mut self, t: Transform) -> Self {
        self.stages.push(t);
        self
    }

    /// Add a filter node.
    pub fn filter(self, predicate: Predicate) -> Self {
        self.stage(Transform::Filter(predicate))
    }

    /// Add a projection node.
    pub fn select(self, columns: Vec<String>) -> Self {
        self.stage(Transform::Select(columns))
    }

    /// Add a derived-column node.
    pub fn derive(self, name: impl Into<String>, expr: Expr, ty: ColumnType) -> Self {
        self.stage(Transform::Derive {
            name: name.into(),
            expr,
            ty,
        })
    }

    /// Add a rename node.
    pub fn rename(self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.stage(Transform::Rename {
            from: from.into(),
            to: to.into(),
        })
    }

    /// Finalise into the shared pipeline IR.
    pub fn build(self) -> Pipeline {
        Pipeline {
            id: self.id,
            source_branch: self.source_branch,
            stages: self.stages,
        }
    }
}
