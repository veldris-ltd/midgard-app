//! Advisory-adapter errors. A missing or malformed advisory is an explicit error,
//! never a silent default — the admin path fails closed and the human proceeds
//! through the guarded path unaided rather than trusting a bad completion.

/// A failure obtaining or parsing an advisory completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdviceError {
    /// The advisor (Mimir bridge or mock) failed to return a completion.
    Advisor(String),
    /// The model response was not valid JSON or did not match the contract.
    Parse(String),
}

impl std::fmt::Display for AdviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdviceError::Advisor(m) => write!(f, "advisory inference failed: {m}"),
            AdviceError::Parse(m) => write!(f, "malformed advisory response: {m}"),
        }
    }
}

impl std::error::Error for AdviceError {}
