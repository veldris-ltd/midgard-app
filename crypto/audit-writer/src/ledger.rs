//! The append-only ledger store and the [`AuditWriter`] that seals records into
//! it. The store trait exposes **only append** (no update/delete) — append-only
//! at the API surface.
//!
//! Scope: this crate provides cryptographic tamper-*evidence*, not filesystem
//! tamper-*prevention*. [`InMemoryLedger`] is the non-durable store for tests/dev;
//! the durable, encrypted, append-only [`crate::file::FileLedger`] (`O_APPEND` +
//! per-entry `fsync` + `0600` perms, entries KMS-sealed at rest) is the production
//! store. Both satisfy the same [`LedgerStore`] trait. [AC-M-4]
//!
//! After every append the writer signs a [`LedgerCheckpoint`] of the ledger's length
//! and head and hands it to the store, which keeps the latest one; a ledger cut
//! short of its checkpoint no longer verifies. [MG-SEC-05]

use pqc_agility::{SignatureScheme, SigningKey, VerifyingKey};

use crate::chain::{entry_hash, genesis_hash, signing_preimage};
use crate::checkpoint::LedgerCheckpoint;
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
    /// Keep `checkpoint` as the ledger's latest signed checkpoint, replacing the
    /// previous one. A durable store must make it durable before returning.
    /// [MG-SEC-05]
    fn record_checkpoint(&mut self, checkpoint: LedgerCheckpoint) -> Result<(), AuditError>;
    /// The latest signed checkpoint, if one has been recorded. [MG-SEC-05]
    fn checkpoint(&self) -> Option<&LedgerCheckpoint>;
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
    fn record_checkpoint(&mut self, checkpoint: LedgerCheckpoint) -> Result<(), AuditError> {
        (**self).record_checkpoint(checkpoint)
    }
    fn checkpoint(&self) -> Option<&LedgerCheckpoint> {
        (**self).checkpoint()
    }
}

/// A non-durable in-memory ledger (tests / dev).
#[derive(Debug, Default)]
pub struct InMemoryLedger {
    entries: Vec<SealedEntry>,
    checkpoint: Option<LedgerCheckpoint>,
}

impl InMemoryLedger {
    /// A new empty ledger.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            checkpoint: None,
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
    fn record_checkpoint(&mut self, checkpoint: LedgerCheckpoint) -> Result<(), AuditError> {
        self.checkpoint = Some(checkpoint);
        Ok(())
    }
    fn checkpoint(&self) -> Option<&LedgerCheckpoint> {
        self.checkpoint.as_ref()
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

    /// The ledger's latest signed checkpoint (for [`crate::verify::verify_ledger`]).
    /// [MG-SEC-05]
    pub fn checkpoint(&self) -> Option<&LedgerCheckpoint> {
        self.store.checkpoint()
    }

    /// Sign and record a checkpoint of the ledger as it stands. [`append`] does this
    /// after every entry; call it directly to vouch for a newly created, still empty
    /// ledger, so that an emptied one can be told from a new one. [MG-SEC-05]
    ///
    /// [`append`]: AuditWriter::append
    pub fn seal_checkpoint(&mut self) -> Result<(), AuditError> {
        let checkpoint = LedgerCheckpoint::sign(
            self.scheme.as_ref(),
            &self.signing_key,
            &self.ledger_id,
            self.store.entries(),
        )?;
        self.store.record_checkpoint(checkpoint)
    }

    /// Seal and append a record, returning its sequence number, then refresh the
    /// signed checkpoint. A checkpoint that cannot be recorded fails the append: the
    /// entry is already durable (one entry ahead of the checkpoint is tolerated on
    /// reopen), but the caller is told, fail-closed. [MG-SEC-05]
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
        self.seal_checkpoint()?;
        Ok(seq)
    }
}
