//! Sync error type. Hand-rolled (no `thiserror`) to keep the crate dependency-free.

use std::fmt;

/// An error from the sync engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    /// An op referenced a replica id that does not match the expected shape.
    MalformedOp(String),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::MalformedOp(d) => write!(f, "malformed sync op: {d}"),
        }
    }
}

impl std::error::Error for SyncError {}
