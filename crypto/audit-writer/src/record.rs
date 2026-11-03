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
    /// Where the request came from, which session made it, how it ended and why.
    /// Empty for an event that knows none of them; an empty context is encoded and
    /// hashed exactly as a record was before the context existed, so ledgers written
    /// then still verify. [VLD-ANB-MIDGARD-0052]
    pub context: AuditContext,
}

/// How a recorded request or event ended. [VLD-ANB-MIDGARD-0052]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Outcome {
    /// Not stated (every record written before the context existed).
    #[default]
    Unspecified,
    /// A policy decision that permitted the request.
    Permit,
    /// A policy decision that denied the request.
    Deny,
    /// Refused before or outside a policy decision: a rejected token, a missing human
    /// gate, a self-approval.
    Refused,
    /// The work was attempted and failed.
    Failed,
    /// An informational event.
    Info,
}

impl Outcome {
    /// The byte the codec and the canonical form carry.
    pub fn as_u8(self) -> u8 {
        match self {
            Outcome::Unspecified => 0,
            Outcome::Permit => 1,
            Outcome::Deny => 2,
            Outcome::Refused => 3,
            Outcome::Failed => 4,
            Outcome::Info => 5,
        }
    }

    /// The outcome a byte names, if any.
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0 => Outcome::Unspecified,
            1 => Outcome::Permit,
            2 => Outcome::Deny,
            3 => Outcome::Refused,
            4 => Outcome::Failed,
            5 => Outcome::Info,
            _ => return None,
        })
    }

    /// The lowercase outcome name.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Unspecified => "unspecified",
            Outcome::Permit => "permit",
            Outcome::Deny => "deny",
            Outcome::Refused => "refused",
            Outcome::Failed => "failed",
            Outcome::Info => "info",
        }
    }
}

/// The structured context of a record: source address, session, outcome, reason
/// code, and for a human gate the approval, its approver and its target. Every field
/// is optional; [`AuditContext::default`] is empty. [VLD-ANB-MIDGARD-0052]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditContext {
    /// The peer address the request came from.
    pub source: Option<String>,
    /// The session (token) the request was made under.
    pub session: Option<String>,
    /// How it ended.
    pub outcome: Outcome,
    /// A short, stable reason code (`human-gate-missing`, `signature`, ...).
    pub reason: Option<String>,
    /// The human-gate approval involved.
    pub approval_id: Option<String>,
    /// Who approved it.
    pub approver: Option<String>,
    /// The object acted on.
    pub target: Option<String>,
}

impl AuditContext {
    /// Whether nothing is stated: the record is then encoded in the original layout.
    pub fn is_empty(&self) -> bool {
        *self == AuditContext::default()
    }

    /// The optional fields, in the order the codec and the canonical form write them.
    pub(crate) fn optional_fields(&self) -> [&Option<String>; 6] {
        [
            &self.source,
            &self.session,
            &self.reason,
            &self.approval_id,
            &self.approver,
            &self.target,
        ]
    }
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
