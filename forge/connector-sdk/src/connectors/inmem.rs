//! A reference **in-memory** connector: wraps an already-built [`Dataset`] as a
//! source. Useful as a test fixture and as the bounded head of a stream in offline
//! runs. Credential-free and network-free. [FR-F-2]

use forge_dataset_store::Dataset;

use crate::connector::{Connector, ConnectorDescriptor, ConnectorKind, SyncContext};
use crate::error::ConnectorError;

/// Serves a fixed in-memory dataset as a connector source.
pub struct InMemoryConnector {
    source: String,
    kind: ConnectorKind,
    streaming: bool,
    dataset: Dataset,
}

impl InMemoryConnector {
    /// A bounded batch source over `dataset`.
    pub fn batch(source: impl Into<String>, dataset: Dataset) -> Self {
        Self {
            source: source.into(),
            kind: ConnectorKind::ObjectStore,
            streaming: false,
            dataset,
        }
    }

    /// A streaming source over `dataset` (its current bounded snapshot). [FR-F-5]
    pub fn stream(source: impl Into<String>, dataset: Dataset) -> Self {
        Self {
            source: source.into(),
            kind: ConnectorKind::Stream,
            streaming: true,
            dataset,
        }
    }
}

impl Connector for InMemoryConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: self.kind,
            source: self.source.clone(),
            streaming: self.streaming,
        }
    }

    fn sync(&self, _ctx: &SyncContext) -> Result<Dataset, ConnectorError> {
        Ok(self.dataset.clone())
    }
}
