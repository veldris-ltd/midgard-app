//! Audit records (SAD_00 §8.2) and the sealed, chained, signed entries stored in
//! the ledger. The SAD's `pq_signature` field lives on [`SealedEntry`], since it
//! signs the chain metadata, not just the payload.

use pqc_agility::{SigId, Signature};

/// Identifier for a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordId(pub String);

/// The actor who performed the action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor(pub String);

/// The action performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action(pub String);

/// The resource acted upon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource(pub String);

/// An RFC 3339 timestamp (supplied by the caller — this crate does no clock I/O).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp(pub String);

/// A ledger identifier; binds the chain's genesis so entries cannot be spliced
/// between ledgers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerId(pub String);

/// A single audit record — the caller's payload. (SAD_00 §8.2)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    /// Record id.
    pub id: RecordId,
    /// Actor.
    pub actor: Actor,
    /// Action.
    pub action: Action,
    /// Resource.
    pub resource: Resource,
    /// Timestamp.
    pub ts: Timestamp,
}

/// A record sealed into the chain: payload + linkage + PQ signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedEntry {
    /// The audit record.
    pub record: AuditRecord,
    /// Monotonic sequence number (0 at genesis).
    pub seq: u64,
    /// Hash of the previous entry (or the genesis hash for seq 0).
    pub prev_hash: [u8; 32],
    /// Hash of this entry.
    pub entry_hash: [u8; 32],
    /// Algorithm of `pq_signature` (recorded so it can rotate per entry).
    pub sig_alg: SigId,
    /// ML-DSA / SLH-DSA signature over the entry hash. [NFR-PQ-3]
    pub pq_signature: Signature,
}
