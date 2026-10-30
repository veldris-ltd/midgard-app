//! Chain verification. Walks from genesis and fails closed on the first defect,
//! naming the offending sequence number. Supports per-entry signature-algorithm
//! rotation via a key resolver (the verifying key can differ per algorithm).
//!
//! [`verify_chain`] proves the entries a ledger still holds. [`verify_ledger`] is
//! the full check a durable ledger is opened under: the chain, its signed
//! checkpoint (so nothing was cut off the end), and both under a pinned key from
//! outside the signing keystore. [MG-SEC-05]

use pqc_agility::{CryptoProvider, SigId, VerifyingKey};

use crate::chain::{entry_hash, genesis_hash, signing_preimage};
use crate::checkpoint::LedgerCheckpoint;
use crate::error::AuditError;
use crate::pin::PinnedKey;
use crate::record::{LedgerId, SealedEntry};

/// Verify an entire chain.
///
/// - `provider` supplies the signature backend for each entry's algorithm.
/// - `ledger_id` must match the one the chain was created with (anti-splice).
/// - `verifying_key_for` resolves the expected public key for a given algorithm;
///   return `None` to reject an entry signed under an unexpected algorithm.
pub fn verify_chain(
    entries: &[SealedEntry],
    provider: &CryptoProvider,
    ledger_id: &LedgerId,
    verifying_key_for: &dyn Fn(SigId) -> Option<VerifyingKey>,
) -> Result<(), AuditError> {
    let mut expected_prev = genesis_hash(ledger_id);
    for (i, e) in entries.iter().enumerate() {
        let expected_seq = i as u64;
        if e.seq != expected_seq {
            return Err(AuditError::SequenceGap {
                expected: expected_seq,
                got: e.seq,
            });
        }
        if e.prev_hash != expected_prev {
            return Err(AuditError::ChainBroken {
                seq: e.seq,
                expected: expected_prev,
                got: e.prev_hash,
            });
        }
        let recomputed = entry_hash(e.seq, &e.prev_hash, &e.record);
        if recomputed != e.entry_hash {
            return Err(AuditError::HashMismatch { seq: e.seq });
        }
        let scheme = provider
            .sig_for(e.sig_alg)
            .map_err(|_| AuditError::UnknownSigAlgorithm { seq: e.seq })?;
        let vk =
            verifying_key_for(e.sig_alg).ok_or(AuditError::UnknownSigAlgorithm { seq: e.seq })?;
        scheme
            .verify(&vk, &signing_preimage(&e.entry_hash), &e.pq_signature)
            .map_err(|_| AuditError::SignatureInvalid { seq: e.seq })?;
        expected_prev = e.entry_hash;
    }
    Ok(())
}

/// Verify a whole ledger: every entry chains and is signed under the **pinned** key,
/// and a checkpoint signed under the same pin vouches for its length and head.
///
/// Fails closed with [`AuditError::CheckpointMissing`] when there is no checkpoint,
/// [`AuditError::Truncated`] when entries were cut off the end,
/// [`AuditError::CheckpointMismatch`] when the tail was rewritten, and
/// [`AuditError::SignatureInvalid`] / [`AuditError::CheckpointInvalid`] when the
/// ledger was signed by any key but the pinned one. [MG-SEC-05]
pub fn verify_ledger(
    entries: &[SealedEntry],
    checkpoint: Option<&LedgerCheckpoint>,
    provider: &CryptoProvider,
    ledger_id: &LedgerId,
    pin: &PinnedKey,
) -> Result<(), AuditError> {
    let checkpoint = checkpoint.ok_or(AuditError::CheckpointMissing)?;
    verify_chain(entries, provider, ledger_id, &|alg| pin.resolve(alg))?;
    checkpoint.verify_signature(provider, ledger_id, pin.key())?;
    checkpoint.check_covers(entries)
}
