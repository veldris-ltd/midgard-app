//! Replication errors. Applying a peer delta fails closed: a bad signature, an
//! algorithm mismatch, or malformed bytes all abort the merge, so an unverified
//! delta never mutates the local picture. [FR-H-6] [NFR-PQ-3]

use pqc_agility::error::CryptoError;
use pqc_agility::SigId;

use heimdall_picture::PictureError;

/// Why a replication step failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    /// The delta signature did not verify against the peer's key — rejected before
    /// any merge. [NFR-PQ-3]
    SignatureInvalid,
    /// The signature's algorithm does not match the peer's key algorithm.
    AlgorithmMismatch {
        /// The peer key's algorithm.
        expected: SigId,
        /// The signature's claimed algorithm.
        got: SigId,
    },
    /// The delta bytes could not be decoded.
    Decode(PictureError),
    /// The crypto provider could not supply a backend for the algorithm. Carries
    /// the provider's message (`CryptoError` is not itself comparable).
    Crypto(String),
    /// The transport link is closed / has no message.
    LinkClosed,
    /// The PQ-mTLS transport (record seal/open or byte framing) failed. Carries the
    /// underlying message (`MtlsError`/`TransportError` are not comparable). [FR-H-6]
    Transport(String),
}

impl From<CryptoError> for SyncError {
    fn from(e: CryptoError) -> Self {
        SyncError::Crypto(format!("{e:?}"))
    }
}

impl From<PictureError> for SyncError {
    fn from(e: PictureError) -> Self {
        SyncError::Decode(e)
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::SignatureInvalid => write!(f, "delta signature failed to verify"),
            SyncError::AlgorithmMismatch { expected, got } => write!(
                f,
                "delta signature algorithm {} does not match peer key algorithm {}",
                got.as_str(),
                expected.as_str()
            ),
            SyncError::Decode(e) => write!(f, "delta decode error: {e}"),
            SyncError::Crypto(e) => write!(f, "crypto backend error: {e}"),
            SyncError::LinkClosed => write!(f, "transport link closed"),
            SyncError::Transport(e) => write!(f, "pq-mtls transport error: {e}"),
        }
    }
}

impl std::error::Error for SyncError {}
