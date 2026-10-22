//! Retention enforcement: the ontology's retention windows applied to the enclosure's
//! durable store. Each sweep removes, per type, the objects whose window has passed,
//! with their links. The removal is audited in the enclosure's one ledger *before*
//! anything is removed, as a sync is, and the removals are journalled (one flush per
//! type), so a restart does not bring them back. [SAD_00 §8.5] [NFR-SEC-2] [AC-M-4]
//!
//! The same upkeep compacts the store's journal once it holds more than
//! [`COMPACT_RATIO`] times the records the data needs (every update appends a whole
//! object), so the journal stays near the size of the data. Compaction changes no
//! object; it is still recorded in the ledger first. A store wedged by a write that
//! could not be cut back off its journal is compacted at the next upkeep whatever its
//! size: that rewrite is how it recovers, so it does not refuse writes until a restart.

use audit_writer::LedgerStore;
use fabric::{DurableObjectStore, StoreOp};

use crate::SecuredGateway;

/// The actor retention sweeps are recorded under.
pub const RETENTION_ACTOR: &str = "governance-retention";
/// The audited action of a sweep.
pub const ACTION_EXPIRE: &str = "RETAIN expire";
/// At most this many objects of one type are removed per sweep, so a large backlog is
/// cleared over several sweeps and the enclosure is not held for long. A type's removals
/// are journalled together and flushed once.
pub const SWEEP_LIMIT: usize = 1000;
/// The audited action of a journal compaction.
pub const ACTION_COMPACT: &str = "COMPACT store";
/// Compact once the journal holds this many times the records the data needs...
pub const COMPACT_RATIO: u64 = 2;
/// ...and at least this many more, so a small store is not rewritten for nothing.
pub const COMPACT_SLACK: u64 = 20_000;

/// What one sweep removed from one object type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expired {
    /// The object type.
    pub object_type: String,
    /// How many of its objects were removed.
    pub removed: usize,
    /// Whether more expired objects of the type remain for the next sweep.
    pub more: bool,
}

impl<L: LedgerStore> SecuredGateway<DurableObjectStore, L> {
    /// Whether the ontology declares any retention window (else sweeping is a no-op).
    pub fn has_retention(&self) -> bool {
        self.ontology
            .as_ref()
            .is_some_and(|o| !o.retention.is_empty())
    }

    /// Compact the store's journal at `now` if it has outgrown its data or the store is
    /// wedged, returning the record counts before and after, or `None` when neither.
    pub fn compact_store_if_due(&self, now: u64) -> Result<Option<(u64, u64)>, String> {
        self.compact_store_with_slack(now, COMPACT_SLACK)
    }

    /// [`Self::compact_store_if_due`] with a chosen slack in place of [`COMPACT_SLACK`].
    pub fn compact_store_with_slack(
        &self,
        now: u64,
        slack: u64,
    ) -> Result<Option<(u64, u64)>, String> {
        let mut enclosure = self
            .enclosure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let store = enclosure.fabric().store();
        let (journal, live) = (store.journal_records(), store.live_records());
        // A wedged store refuses every write until its journal is rewritten from
        // memory, so it is compacted whatever its size.
        let wedged = store.wedged().is_some();
        if !wedged && journal <= live.saturating_mul(COMPACT_RATIO) + slack {
            return Ok(None);
        }
        let why = if wedged { ":wedged" } else { "" };
        enclosure
            .governance_mut()
            .record_event(
                RETENTION_ACTOR,
                ACTION_COMPACT,
                &format!("store:{journal}-records:to-{live}{why}"),
                now,
            )
            .map_err(|e| format!("the compaction could not be audited: {e}"))?;
        enclosure
            .fabric_mut()
            .store_mut()
            .compact()
            .map(Some)
            .map_err(|e| format!("the compaction failed; the journal is unchanged: {e}"))
    }

    /// One retention sweep at `now`. A type whose sweep cannot be audited is left as it
    /// is, and the error says which.
    pub fn enforce_retention(&self, now: u64) -> Result<Vec<Expired>, String> {
        let Some(ontology) = self.ontology.as_ref() else {
            return Ok(Vec::new());
        };
        let mut enclosure = self
            .enclosure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut out = Vec::new();
        for rule in &ontology.retention {
            let mut ids = rule.expired(enclosure.fabric().store(), now, SWEEP_LIMIT + 1);
            let more = ids.len() > SWEEP_LIMIT;
            ids.truncate(SWEEP_LIMIT);
            if ids.is_empty() {
                continue;
            }
            let hours = rule.seconds / 3600;
            enclosure
                .governance_mut()
                .record_event(
                    RETENTION_ACTOR,
                    ACTION_EXPIRE,
                    &format!(
                        "retention:{}:{}-removed:after-{hours}h:oldest-{}",
                        rule.object_type,
                        ids.len(),
                        ids[0]
                    ),
                    now,
                )
                .map_err(|e| format!("the {} sweep could not be audited: {e}", rule.object_type))?;
            // One batch per type: journalled together, flushed once.
            let removed = enclosure
                .fabric_mut()
                .store_mut()
                .apply_batch(ids.into_iter().map(StoreOp::RemoveObject).collect())
                .map_err(|e| {
                    format!(
                        "the {} sweep could not be written; nothing was removed, and the \
                         ledger records it: {e}",
                        rule.object_type
                    )
                })?;
            out.push(Expired {
                object_type: rule.object_type.clone(),
                removed,
                more,
            });
        }
        Ok(out)
    }
}
