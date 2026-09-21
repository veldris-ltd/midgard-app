//! The append-only admin audit log: an [`AdminEvent`] recorder over a PQ-signed
//! `audit-writer` chain.

use audit_writer::{
    Action, Actor, AuditRecord, AuditWriter, LedgerId, LedgerStore, RecordId, Resource,
    SealedEntry, Timestamp,
};
use pqc_agility::VerifyingKey;

use crate::error::AdminAuditError;
use crate::event::AdminEvent;

/// Records admin events into a chained, ML-DSA-signed, append-only ledger. Keeps
/// the decoded [`AdminEvent`]s alongside the sealed chain for ergonomic inspection;
/// the authoritative, tamper-evident copy is the signed chain.
pub struct AdminAuditLog<S: LedgerStore> {
    writer: AuditWriter<S>,
    events: Vec<AdminEvent>,
}

impl<S: LedgerStore> AdminAuditLog<S> {
    /// Build a log over an audit writer.
    pub fn new(writer: AuditWriter<S>) -> Self {
        Self {
            writer,
            events: Vec::new(),
        }
    }

    /// Record one admin event, sealing it into the chain. Fails closed (nothing is
    /// appended) if the event is malformed or the ledger rejects it. Returns the
    /// entry's sequence number. `[AC-AD-6]`
    pub fn record(&mut self, event: AdminEvent, ts: &str) -> Result<u64, AdminAuditError> {
        let detail = event.encode()?;
        let record = AuditRecord {
            // Unique per entry: the action verb plus a monotonic index.
            id: RecordId(format!("{}:{}", event.what, self.events.len())),
            actor: Actor(event.who.clone()),
            action: Action(event.what.clone()),
            resource: Resource(detail),
            ts: Timestamp(ts.to_string()),
        };
        let seq = self
            .writer
            .append(record)
            .map_err(|e| AdminAuditError::Ledger(format!("{e:?}")))?;
        self.events.push(event);
        Ok(seq)
    }

    /// The recorded events, in order.
    pub fn events(&self) -> &[AdminEvent] {
        &self.events
    }

    /// The sealed audit entries (the tamper-evident chain).
    pub fn entries(&self) -> &[SealedEntry] {
        self.writer.entries()
    }

    /// The verifying key for the chain (for `audit_writer::verify_chain`).
    pub fn verifying_key(&self) -> &VerifyingKey {
        self.writer.verifying_key()
    }

    /// The ledger id.
    pub fn ledger_id(&self) -> &LedgerId {
        self.writer.ledger_id()
    }
}
