//! Picture-model errors. Decoding canonical bytes fails closed: a malformed or
//! truncated buffer never surfaces as a partial value.

/// A model-level failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PictureError {
    /// Canonical bytes could not be decoded (truncated, unknown tag, bad UTF-8).
    Decode(String),
}

impl std::fmt::Display for PictureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PictureError::Decode(m) => write!(f, "decode error: {m}"),
        }
    }
}

impl std::error::Error for PictureError {}
