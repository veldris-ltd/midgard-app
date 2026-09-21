//! Feed-authentication errors. Every variant is a rejection: there is no code path
//! that admits an observation without a verified signature. [FR-H-3]

use pqc_agility::error::CryptoError;
use pqc_agility::SigId;

use heimdall_picture::FeedId;

/// Why an observation was rejected from the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    /// The observation's feed is not registered with a verifying key.
    UnknownFeed(FeedId),
    /// The signature's algorithm does not match the feed's registered key.
    AlgorithmMismatch {
        /// The algorithm of the registered feed key.
        expected: SigId,
        /// The algorithm the signature claimed.
        got: SigId,
    },
    /// The signature did not verify against the feed's key and the observation
    /// bytes (spoofed, tampered, or wrong key). [NFR-PQ-3]
    SignatureInvalid(FeedId),
    /// The provider could not supply a backend for the signature algorithm
    /// (unregistered or disabled) — fails closed. Carries the provider's message
    /// (`CryptoError` is not itself comparable).
    Crypto(String),
}

impl From<CryptoError> for FeedError {
    fn from(e: CryptoError) -> Self {
        FeedError::Crypto(format!("{e:?}"))
    }
}

impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedError::UnknownFeed(id) => write!(f, "unknown feed '{}'", id.as_str()),
            FeedError::AlgorithmMismatch { expected, got } => write!(
                f,
                "signature algorithm {} does not match feed key algorithm {}",
                got.as_str(),
                expected.as_str()
            ),
            FeedError::SignatureInvalid(id) => {
                write!(f, "signature failed to verify for feed '{}'", id.as_str())
            }
            FeedError::Crypto(e) => write!(f, "crypto backend error: {e}"),
        }
    }
}

impl std::error::Error for FeedError {}
