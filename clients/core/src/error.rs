//! Client-core error type — the single façade error every shell sees. [FR-C-1]

use std::fmt;

use midgard_client_store::StoreError;
use midgard_client_sync::SyncError;

/// An error from the composed client core.
#[derive(Debug)]
pub enum CoreError {
    /// A local-store (crypto / clearance / custody) error.
    Store(StoreError),
    /// A remote op blob handed across the binding did not decode — rejected,
    /// never applied to the CRDT. [RT-58]
    MalformedOp(String),
    /// A persona switch with unsent ops: push them (or drop them) first. `[FR-C-4]`
    PendingOps(usize),
    /// Cached state opened but did not parse (a collection index or the pending
    /// queue). Nothing of it was loaded.
    Malformed(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Store(e) => write!(f, "{e}"),
            CoreError::MalformedOp(d) => write!(f, "malformed sync op: {d}"),
            CoreError::PendingOps(n) => write!(f, "{n} unsent op(s); sync first"),
            CoreError::Malformed(d) => write!(f, "cached state malformed: {d}"),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<StoreError> for CoreError {
    fn from(e: StoreError) -> Self {
        CoreError::Store(e)
    }
}

impl From<SyncError> for CoreError {
    fn from(e: SyncError) -> Self {
        CoreError::MalformedOp(e.to_string())
    }
}
