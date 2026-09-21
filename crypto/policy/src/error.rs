//! Policy loading/validation errors. Evaluation itself is total (never errors)
//! so a caller cannot mistake an error for a permit.

use core::fmt;

/// Errors raised while *constructing* a policy set — never during evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PolicyError {
    /// Two rules share the same id.
    DuplicateRuleId(String),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyError::DuplicateRuleId(id) => write!(f, "duplicate rule id: {id}"),
        }
    }
}

impl std::error::Error for PolicyError {}
