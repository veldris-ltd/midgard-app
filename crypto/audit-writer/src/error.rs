//! Audit ledger errors. Every verification failure names the offending sequence
//! number and is fail-closed.

use core::fmt;
use pqc_agility::CryptoError;

/// Errors from writing or verifying the audit ledger.
#[derive(Debug)]
#[non_exhaustive]
pub enum AuditError {
    /// The underlying store failed.
    Store(String),
    /// The ledger was unexpectedly empty.
    Empty,
    /// Sequence numbers are not contiguous from genesis.
    SequenceGap { expected: u64, got: u64 },
    /// An entry's `prev_hash` does not match the previous entry's hash.
    ChainBroken {
        seq: u64,
        expected: [u8; 32],
        got: [u8; 32],
    },
    /// A recomputed entry hash does not match the stored one (record tampered).
    HashMismatch { seq: u64 },
    /// An entry's signature failed verification.
    SignatureInvalid { seq: u64 },
    /// No verifying key / backend was available for an entry's algorithm.
    UnknownSigAlgorithm { seq: u64 },
    /// A cryptographic operation failed.
    Crypto(CryptoError),
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuditError::Store(s) => write!(f, "ledger store error: {s}"),
            AuditError::Empty => write!(f, "ledger is empty"),
            AuditError::SequenceGap { expected, got } => {
                write!(f, "sequence gap: expected {expected}, got {got}")
            }
            AuditError::ChainBroken { seq, .. } => {
                write!(f, "chain broken at seq {seq}: prev_hash mismatch")
            }
            AuditError::HashMismatch { seq } => write!(f, "entry hash mismatch at seq {seq}"),
            AuditError::SignatureInvalid { seq } => write!(f, "invalid signature at seq {seq}"),
            AuditError::UnknownSigAlgorithm { seq } => {
                write!(f, "no verifying key for algorithm at seq {seq}")
            }
            AuditError::Crypto(e) => write!(f, "crypto error: {e}"),
        }
    }
}

impl std::error::Error for AuditError {}

impl From<CryptoError> for AuditError {
    fn from(e: CryptoError) -> Self {
        AuditError::Crypto(e)
    }
}
