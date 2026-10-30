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
//!
//! A chain alone cannot show that nothing was cut off its end, so every append also
//! refreshes a signed [`LedgerCheckpoint`] of the ledger's length and head; a durable
//! ledger must exist to be opened; and [`verify::verify_ledger`] checks chain and
//! checkpoint under a [`PinnedKey`] supplied from outside the signing keystore.
//! [MG-SEC-05]

pub mod canonical;
pub mod chain;
pub mod checkpoint;
pub mod codec;
pub mod error;
pub mod file;
pub mod journal;
pub mod ledger;
pub mod pin;
pub mod record;
pub mod verify;

pub use canonical::canonical_record;
pub use chain::{checkpoint_preimage, entry_hash, genesis_hash, signing_preimage};
pub use checkpoint::LedgerCheckpoint;
pub use codec::{decode_sealed_entry, encode_sealed_entry};
pub use error::AuditError;
pub use file::{checkpoint_path, FileLedger, Sealer};
pub use journal::SealedJournal;
pub use ledger::{AuditWriter, InMemoryLedger, LedgerStore};
pub use pin::PinnedKey;
pub use record::{
    Action, Actor, AuditRecord, LedgerId, RecordId, Resource, SealedEntry, Timestamp,
};
pub use verify::{verify_chain, verify_ledger};
