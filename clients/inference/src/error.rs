//! Inference-adapter error type.

use std::fmt;

/// An error from the client inference adapter. Every variant fails closed — no
/// path returns a partial or ungrounded result as success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceError {
    /// A Tier-1 (on-device) response was requested but no verified model is loaded.
    NoOnDeviceModel,
    /// A remote (Tier-2/3) response was requested but the enclosure is unreachable.
    EnclosureUnreachable,
    /// The on-device model's PQ signature did not verify — refuse to load. `[AC-C-10]`
    ModelSignatureInvalid,
    /// No signature backend is available to verify a model.
    SignatureBackendUnavailable,
    /// A remote result came back ungrounded/uncited — refused. `[AC-C-6]`
    Ungrounded,
    /// The on-device runtime failed to produce a result.
    Runtime(String),
}

impl fmt::Display for InferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InferenceError::NoOnDeviceModel => write!(f, "no verified on-device model is loaded"),
            InferenceError::EnclosureUnreachable => write!(f, "enclosure is unreachable"),
            InferenceError::ModelSignatureInvalid => {
                write!(f, "on-device model signature invalid; refusing to load")
            }
            InferenceError::SignatureBackendUnavailable => {
                write!(f, "no signature backend available to verify the model")
            }
            InferenceError::Ungrounded => write!(f, "remote result was ungrounded/uncited"),
            InferenceError::Runtime(d) => write!(f, "on-device runtime failed: {d}"),
        }
    }
}

impl std::error::Error for InferenceError {}
