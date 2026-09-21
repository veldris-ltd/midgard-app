//! # audit-writer — PQ-signed, append-only, tamper-evident audit ledger
//!
//! Every entry is chained to the previous by hash and signed with a post-quantum
//! signature (`[NFR-PQ-3]`), so any mutation, reorder, gap, or cross-ledger
//! splice is detected by [`verify::verify_chain`]. This is the concrete backing
//! for envelope invariant INV-6 (Audited) and acceptance criterion AC-M-4.
//!
//! The chain hash reuses `pqc-agility`'s KAT-tested SHA-256 (no second hash
//! implementation), and signing goes through the same agility shim as the rest of
//! the platform, so audit signatures rotate algorithms with no data migration.

pub mod canonical;
pub mod chain;
pub mod codec;
pub mod error;
pub mod file;
pub mod journal;
pub mod ledger;
pub mod record;
pub mod verify;

pub use canonical::canonical_record;
pub use chain::{entry_hash, genesis_hash, signing_preimage};
pub use codec::{decode_sealed_entry, encode_sealed_entry};
pub use error::AuditError;
pub use file::{FileLedger, Sealer};
pub use journal::SealedJournal;
pub use ledger::{AuditWriter, InMemoryLedger, LedgerStore};
pub use record::{
    Action, Actor, AuditRecord, LedgerId, RecordId, Resource, SealedEntry, Timestamp,
};
pub use verify::verify_chain;
