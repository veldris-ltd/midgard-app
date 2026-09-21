//! AI-assist errors. A malformed or absent suggestion never becomes a silent
//! no-op transform — it surfaces as an explicit error the caller must handle.

/// A failure assembling, calling, or parsing an AI transform suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistError {
    /// The suggester (Mimir bridge or mock) failed to return a completion.
    Suggest(String),
    /// The model response was not valid JSON, or did not match the A.6 contract.
    Parse(String),
}

impl std::fmt::Display for AssistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssistError::Suggest(m) => write!(f, "inference failed: {m}"),
            AssistError::Parse(m) => write!(f, "malformed A.6 response: {m}"),
        }
    }
}

impl std::error::Error for AssistError {}
