//! Decision-support errors. The adapter fails closed: if inference is unavailable
//! or the response is malformed, it returns an error rather than fabricating an
//! option. [FR-H-4]

/// A decision-support failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionError {
    /// The inference call failed (Mimir unreachable, timeout, refusal).
    Infer(String),
    /// The model response could not be parsed into a valid option set.
    Parse(String),
}

impl std::fmt::Display for DecisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecisionError::Infer(m) => write!(f, "inference error: {m}"),
            DecisionError::Parse(m) => write!(f, "parse error: {m}"),
        }
    }
}

impl std::error::Error for DecisionError {}
