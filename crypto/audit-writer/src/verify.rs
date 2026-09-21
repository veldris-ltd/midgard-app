//! Chain verification. Walks from genesis and fails closed on the first defect,
//! naming the offending sequence number. Supports per-entry signature-algorithm
//! rotation via a key resolver (the verifying key can differ per algorithm).

use pqc_agility::{CryptoProvider, SigId, VerifyingKey};

use crate::chain::{entry_hash, genesis_hash, signing_preimage};
use crate::error::AuditError;
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
