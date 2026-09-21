//! Fail-closed errors for the durable Heimdall stores. [FR-H-5]

/// A durable-store failure: I/O, seal/unseal, or a malformed/tampered on-disk blob.
#[derive(Debug)]
pub enum StoreError {
    /// Filesystem I/O failed.
    Io(String),
    /// Sealing or unsealing (KMS envelope) failed — includes at-rest tamper.
    Seal(String),
    /// A recovered blob did not decode into valid state (corruption / version skew).
    Decode(String),
    /// A workflow transition failed.
    Workflow(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(m) => write!(f, "durable store i/o: {m}"),
            StoreError::Seal(m) => write!(f, "seal/unseal failed: {m}"),
            StoreError::Decode(m) => write!(f, "decode failed: {m}"),
            StoreError::Workflow(m) => write!(f, "workflow: {m}"),
        }
    }
}

impl std::error::Error for StoreError {}
