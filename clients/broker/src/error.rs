//! Broker error type.

use std::fmt;

/// An error from the extension broker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerError {
    /// The delegated (brokered) assistant call failed.
    Assistant(String),
}

impl fmt::Display for BrokerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrokerError::Assistant(d) => write!(f, "brokered assistant call failed: {d}"),
        }
    }
}

impl std::error::Error for BrokerError {}
