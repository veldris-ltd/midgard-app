//! Data lineage: the immutable "where did this come from" record attached to every
//! committed version. Recorded at each hop (ingest → transform → curate) so the
//! full provenance of a dataset — and everything projected from it — is auditable.
//! [NFR-SEC-2]

use crate::version::ContentId;

/// The provenance of one committed version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lineage {
    /// The origin of the data, e.g. `connector:file:sales.csv` for a raw ingest or
    /// `pipeline:daily-revenue` for a transform output.
    pub source: String,
    /// The operation that produced this version, e.g. `sync`, `transform:filter`,
    /// `transform:derive`.
    pub operation: String,
    /// The upstream version(s) this one was derived from (content-addressed).
    pub inputs: Vec<ContentId>,
}

impl Lineage {
    /// Lineage for a raw ingest (no upstream dataset versions).
    pub fn ingest(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            operation: "sync".into(),
            inputs: Vec::new(),
        }
    }

    /// Lineage for a transform output derived from one or more inputs.
    pub fn transform(
        source: impl Into<String>,
        operation: impl Into<String>,
        inputs: Vec<ContentId>,
    ) -> Self {
        Self {
            source: source.into(),
            operation: operation.into(),
            inputs,
        }
    }
}
