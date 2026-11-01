//! Hash-chain primitives. Domain-separated so a genesis hash, an entry hash, and
//! a signing preimage can never collide.

use pqc_agility::hash::{Digest, Sha256};

use crate::canonical::canonical_record;
use crate::record::{AuditRecord, LedgerId};

const DS_GENESIS: &[u8] = b"midgard-audit-genesis-v1";
const DS_ENTRY: &[u8] = b"midgard-audit-entry-v1";
const DS_SIG: &[u8] = b"midgard-audit-sig-v1";
const DS_CHECKPOINT: &[u8] = b"midgard-audit-checkpoint-v1";

/// The genesis `prev_hash`, bound to the ledger id (anti-splice).
pub fn genesis_hash(ledger: &LedgerId) -> [u8; 32] {
    let mut d = Sha256::new();
    d.update(DS_GENESIS);
    d.update(ledger.0.as_bytes());
    d.finalize()
}

/// The hash of an entry: `H(DS_ENTRY ‖ seq ‖ prev_hash ‖ canonical(record))`.
/// The sequence number is inside the hash so reordering/gaps are detectable.
pub fn entry_hash(seq: u64, prev_hash: &[u8; 32], record: &AuditRecord) -> [u8; 32] {
    let mut d = Sha256::new();
    d.update(DS_ENTRY);
    d.update(&seq.to_le_bytes());
    d.update(prev_hash);
    d.update(&canonical_record(record));
    d.finalize()
}

/// The preimage that gets signed: `DS_SIG ‖ entry_hash`.
pub fn signing_preimage(entry_hash: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(DS_SIG.len() + 32);
    v.extend_from_slice(DS_SIG);
    v.extend_from_slice(entry_hash);
    v
}

/// The preimage a ledger checkpoint signs:
/// `DS_CHECKPOINT ‖ len(ledger_id) ‖ ledger_id ‖ count ‖ head_hash`. Its own domain
/// tag, so a checkpoint signature can never be passed off as an entry signature (or
/// the reverse), and the ledger id is length-prefixed so it cannot bleed into the
/// count. [MG-SEC-05]
pub fn checkpoint_preimage(ledger: &LedgerId, count: u64, head_hash: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(DS_CHECKPOINT.len() + 4 + ledger.0.len() + 8 + 32);
    v.extend_from_slice(DS_CHECKPOINT);
    v.extend_from_slice(&(ledger.0.len() as u32).to_le_bytes());
    v.extend_from_slice(ledger.0.as_bytes());
    v.extend_from_slice(&count.to_le_bytes());
    v.extend_from_slice(head_hash);
    v
}
