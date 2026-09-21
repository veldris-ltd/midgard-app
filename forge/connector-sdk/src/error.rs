//! Connector error taxonomy. A connector failure never yields a partial dataset —
//! every path returns an explicit error, so a broken source cannot silently commit
//! garbage into the store.

use forge_dataset_store::{DataError, StoreError};

/// A connector or credential failure.
#[derive(Debug)]
pub enum ConnectorError {
    /// The connector asked to reach a host the sandbox does not permit. In the
    /// air-gapped default this is any outbound access. [NFR-PRV-1]
    SandboxDenied {
        /// The host the connector tried to reach.
        host: String,
    },
    /// A required credential could not be resolved (unknown ref, or the KMS refused
    /// to unseal it).
    Credential(String),
    /// The source payload could not be parsed into the declared schema.
    Parse(String),
    /// The parsed rows did not honour the declared schema.
    Data(DataError),
    /// Committing the sealed raw dataset to the store failed.
    Store(String),
    /// Reading the source (file / local object) failed. [FR-F-2]
    Io(String),
}

impl std::fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectorError::SandboxDenied { host } => {
                write!(f, "sandbox denied egress to '{host}'")
            }
            ConnectorError::Credential(m) => write!(f, "credential error: {m}"),
            ConnectorError::Parse(m) => write!(f, "parse error: {m}"),
            ConnectorError::Data(e) => write!(f, "schema violation: {e}"),
            ConnectorError::Store(m) => write!(f, "store error: {m}"),
            ConnectorError::Io(m) => write!(f, "source I/O error: {m}"),
        }
    }
}

impl std::error::Error for ConnectorError {}

impl From<DataError> for ConnectorError {
    fn from(e: DataError) -> Self {
        ConnectorError::Data(e)
    }
}

impl From<StoreError> for ConnectorError {
    fn from(e: StoreError) -> Self {
        ConnectorError::Store(e.to_string())
    }
}
