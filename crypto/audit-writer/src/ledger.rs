//! The append-only ledger store and the [`AuditWriter`] that seals records into
//! it. The store trait exposes **only append** (no update/delete) — append-only
//! at the API surface.
//!
//! Scope: this crate provides cryptographic tamper-*evidence*, not filesystem
//! tamper-*prevention*. [`InMemoryLedger`] is the non-durable store for tests/dev;
//! the durable, encrypted, append-only [`crate::file::FileLedger`] (`O_APPEND` +
//! per-entry `fsync` + `0600` perms, entries KMS-sealed at rest) is the production
//! store. Both satisfy the same [`LedgerStore`] trait. [AC-M-4]

use pqc_agility::{SignatureScheme, SigningKey, VerifyingKey};

use crate::chain::{entry_hash, genesis_hash, signing_preimage};
use crate::error::AuditError;
use crate::record::{AuditRecord, LedgerId, SealedEntry};

/// An append-only store of sealed entries.
pub trait LedgerStore {
    /// Append one sealed entry.
    fn append(&mut self, entry: SealedEntry) -> Result<(), AuditError>;
    /// The most recent entry, if any.
    fn last(&self) -> Option<&SealedEntry>;
    /// All entries in order.
    fn entries(&self) -> &[SealedEntry];
}

/// A boxed ledger is a ledger, so a service can choose in-memory or durable storage
/// at startup behind one concrete writer type. [AC-M-4]
impl<T: LedgerStore + ?Sized> LedgerStore for Box<T> {
    fn append(&mut self, entry: SealedEntry) -> Result<(), AuditError> {
        (**self).append(entry)
    }
    fn last(&self) -> Option<&SealedEntry> {
        (**self).last()
    }
    fn entries(&self) -> &[SealedEntry] {
        (**self).entries()
    }
}

/// A non-durable in-memory ledger (tests / dev).
#[derive(Debug, Default)]
pub struct InMemoryLedger {
    entries: Vec<SealedEntry>,
}

impl InMemoryLedger {
    /// A new empty ledger.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl LedgerStore for InMemoryLedger {
    fn append(&mut self, entry: SealedEntry) -> Result<(), AuditError> {
        self.entries.push(entry);
        Ok(())
    }
    fn last(&self) -> Option<&SealedEntry> {
        self.entries.last()
    }
    fn entries(&self) -> &[SealedEntry] {
        &self.entries
    }
}

/// Seals audit records into a chained, PQ-signed ledger.
pub struct AuditWriter<S: LedgerStore> {
    store: S,
    scheme: Box<dyn SignatureScheme>,
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    ledger_id: LedgerId,
}

impl<S: LedgerStore> AuditWriter<S> {
    /// Build a writer over a store and a signer.
    pub fn new(
        store: S,
        scheme: Box<dyn SignatureScheme>,
        signing_key: SigningKey,
        verifying_key: VerifyingKey,
        ledger_id: LedgerId,
    ) -> Self {
        Self {
            store,
            scheme,
            signing_key,
            verifying_key,
            ledger_id,
        }
    }

    /// The verifying key for this writer's signer.
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// The ledger id.
    pub fn ledger_id(&self) -> &LedgerId {
        &self.ledger_id
    }

    /// All sealed entries so far.
    pub fn entries(&self) -> &[SealedEntry] {
        self.store.entries()
    }

    /// Seal and append a record, returning its sequence number.
    pub fn append(&mut self, record: AuditRecord) -> Result<u64, AuditError> {
        let (seq, prev_hash) = match self.store.last() {
            Some(e) => (e.seq + 1, e.entry_hash),
            None => (0, genesis_hash(&self.ledger_id)),
        };
        let eh = entry_hash(seq, &prev_hash, &record);
        let sig = self
            .scheme
            .sign(&self.signing_key, &signing_preimage(&eh))?;
        let entry = SealedEntry {
            record,
            seq,
            prev_hash,
            entry_hash: eh,
            sig_alg: self.signing_key.alg(),
            pq_signature: sig,
        };
        self.store.append(entry)?;
        Ok(seq)
    }
}
