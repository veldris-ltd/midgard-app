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
    /// The ledger file is not there. An existing ledger is opened, never silently
    /// re-created: deleting it is tampering, not a fresh start. [MG-SEC-05]
    LedgerMissing(String),
    /// A new ledger was asked for where one (or its checkpoint) already exists.
    LedgerExists(String),
    /// The ledger has no signed checkpoint, so its length cannot be vouched for.
    /// [MG-SEC-05]
    CheckpointMissing,
    /// The checkpoint is malformed, names another ledger, or does not verify under
    /// the pinned key. [MG-SEC-05]
    CheckpointInvalid(String),
    /// The ledger holds fewer entries than its signed checkpoint vouches for: entries
    /// were cut off the end. [MG-SEC-05]
    Truncated { checkpoint: u64, found: u64 },
    /// The entry the checkpoint names as the head is not the one in the ledger: the
    /// tail was rewritten. [MG-SEC-05]
    CheckpointMismatch { seq: u64 },
    /// The ledger is more than one entry ahead of its checkpoint. Only the window
    /// between an entry's `fsync` and its checkpoint's can leave it one ahead.
    /// [MG-SEC-05]
    CheckpointStale { checkpoint: u64, found: u64 },
    /// The key the ledger is checked with is not the pinned audit key. [MG-SEC-05]
    KeyNotPinned,
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
            AuditError::LedgerMissing(p) => write!(f, "the ledger {p} is missing"),
            AuditError::LedgerExists(p) => write!(f, "a ledger already exists at {p}"),
            AuditError::CheckpointMissing => write!(f, "the ledger has no signed checkpoint"),
            AuditError::CheckpointInvalid(why) => write!(f, "the ledger checkpoint is invalid: {why}"),
            AuditError::Truncated { checkpoint, found } => write!(
                f,
                "the ledger was truncated: its checkpoint vouches for {checkpoint} entries,                  {found} remain"
            ),
            AuditError::CheckpointMismatch { seq } => {
                write!(f, "the ledger's entry at seq {seq} is not the checkpointed head")
            }
            AuditError::CheckpointStale { checkpoint, found } => write!(
                f,
                "the ledger holds {found} entries but its checkpoint only {checkpoint}"
            ),
            AuditError::KeyNotPinned => write!(f, "the audit key is not the pinned audit key"),
        }
    }
}

impl std::error::Error for AuditError {}

impl From<CryptoError> for AuditError {
    fn from(e: CryptoError) -> Self {
        AuditError::Crypto(e)
    }
}
