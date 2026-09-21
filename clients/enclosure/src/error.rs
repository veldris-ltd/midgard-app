//! Enclosure-client error type.

use std::fmt;

use pq_mtls::MtlsError;

/// An error opening a session to the enclosure. Every variant is a fail-closed
/// outcome — none yields a usable session.
#[derive(Debug)]
pub enum EnclosureError {
    /// The PQ-mTLS handshake failed (no common PQ suite, bad cert, downgrade,
    /// signature/key-confirmation failure, or authorization denied).
    Handshake(MtlsError),
    /// The handshake authenticated a server other than the pinned enclosure peer.
    PeerPinMismatch {
        /// The pinned gateway identity.
        expected: String,
        /// The identity actually presented.
        actual: String,
    },
}

impl fmt::Display for EnclosureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnclosureError::Handshake(e) => write!(f, "PQ-mTLS handshake failed closed: {e}"),
            EnclosureError::PeerPinMismatch { expected, actual } => {
                write!(f, "peer pin mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for EnclosureError {}
