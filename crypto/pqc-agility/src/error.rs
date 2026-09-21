//! Error types for the crypto-agility shim.

use core::fmt;

/// Errors from KEM / signature / backend operations. `#[non_exhaustive]` so new
/// variants can be added without breaking downstream matches.
#[derive(Debug)]
#[non_exhaustive]
pub enum CryptoError {
    /// Algorithm id is not registered in the provider.
    UnknownAlgorithm(&'static str),
    /// Algorithm is registered but currently disabled (crypto-agility). [NFR-PQ-4]
    AlgorithmDisabled(&'static str),
    /// No backend is available for the requested algorithm (fail closed). [AC-M-1]
    BackendUnavailable(&'static str),
    /// An artefact's embedded algorithm id does not match the key/context.
    AlgorithmMismatch {
        expected: &'static str,
        got: &'static str,
    },
    /// Key material is malformed or the wrong length.
    InvalidKey,
    /// Ciphertext is malformed or the wrong length.
    InvalidCiphertext,
    /// Signature verification failed.
    InvalidSignature,
    /// KEM decapsulation failed.
    DecapsulationFailed,
    /// AEAD seal/open failed (authentication-tag mismatch or malformed input).
    AeadFailed,
    /// The randomness source failed.
    RngFailure,
    /// A byte buffer had an unexpected length.
    LengthMismatch { expected: usize, got: usize },
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CryptoError::UnknownAlgorithm(a) => write!(f, "unknown algorithm: {a}"),
            CryptoError::AlgorithmDisabled(a) => write!(f, "algorithm disabled: {a}"),
            CryptoError::BackendUnavailable(a) => write!(f, "backend unavailable for: {a}"),
            CryptoError::AlgorithmMismatch { expected, got } => {
                write!(f, "algorithm mismatch: expected {expected}, got {got}")
            }
            CryptoError::InvalidKey => write!(f, "invalid key material"),
            CryptoError::InvalidCiphertext => write!(f, "invalid ciphertext"),
            CryptoError::InvalidSignature => write!(f, "signature verification failed"),
            CryptoError::DecapsulationFailed => write!(f, "decapsulation failed"),
            CryptoError::AeadFailed => write!(f, "AEAD operation failed"),
            CryptoError::RngFailure => write!(f, "randomness source failed"),
            CryptoError::LengthMismatch { expected, got } => {
                write!(f, "length mismatch: expected {expected}, got {got}")
            }
        }
    }
}

impl std::error::Error for CryptoError {}

/// Errors from algorithm negotiation. Every variant is a fail-closed outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NegotiationError {
    /// The peer offered no algorithms at all.
    EmptyPeerSet,
    /// The peer offered algorithms, but none overlap with the enabled local set.
    NoCommonAlgorithm,
    /// The peer offered only non-post-quantum algorithms. [AC-M-1]
    ///
    /// Note: because [`crate::algorithm::KemSuiteId`] enumerates only PQ/hybrid
    /// suites by construction, a purely classical offer is unrepresentable and
    /// reduces to [`NegotiationError::EmptyPeerSet`]. This variant exists for
    /// callers that model classical offers out-of-band.
    NoPostQuantumOffered,
}

impl fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NegotiationError::EmptyPeerSet => write!(f, "peer offered no algorithms"),
            NegotiationError::NoCommonAlgorithm => write!(f, "no common algorithm"),
            NegotiationError::NoPostQuantumOffered => {
                write!(f, "peer offered no post-quantum algorithm")
            }
        }
    }
}

impl std::error::Error for NegotiationError {}
