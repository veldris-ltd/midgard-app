//! Client-delivery error type.

use std::fmt;

use bifrost_artefact::VerifyError;
use bifrost_bundle::BundleError;

/// An error installing or updating a client artefact. Every variant is fail-closed.
#[derive(Debug)]
pub enum DeliveryError {
    /// The artefact's signature or content digests did not verify.
    Verify(VerifyError),
    /// An offline bundle failed to import (bad signature / manifest mismatch / a
    /// contained artefact failed).
    Bundle(BundleError),
    /// The artefact is not the `client` component (e.g. a model bundle).
    WrongComponent {
        /// The component the client expected.
        expected: String,
        /// The component the artefact declared.
        actual: String,
    },
    /// An update targeted a different artefact id than what is installed.
    WrongArtefact {
        /// The installed artefact id.
        expected: String,
        /// The update's artefact id.
        actual: String,
    },
}

impl fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeliveryError::Verify(e) => write!(f, "artefact verification failed: {e}"),
            DeliveryError::Bundle(e) => write!(f, "offline bundle import failed: {e}"),
            DeliveryError::WrongComponent { expected, actual } => {
                write!(f, "wrong component: expected {expected}, got {actual}")
            }
            DeliveryError::WrongArtefact { expected, actual } => {
                write!(f, "wrong artefact: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for DeliveryError {}
