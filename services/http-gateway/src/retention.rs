//! Retention enforcement: the ontology's retention windows applied to the enclosure's
//! durable store. Each sweep removes, per type, the objects whose window has passed,
//! with their links. The removal is audited in the enclosure's one ledger *before*
//! anything is removed, as a sync is, and each removal is journalled, so a restart
//! does not bring them back. [SAD_00 §8.5] [NFR-SEC-2] [AC-M-4]

use audit_writer::LedgerStore;
use fabric::DurableObjectStore;

use crate::SecuredGateway;

/// The actor retention sweeps are recorded under.
pub const RETENTION_ACTOR: &str = "governance-retention";
/// The audited action of a sweep.
pub const ACTION_EXPIRE: &str = "RETAIN expire";
/// At most this many objects of one type are removed per sweep, so a large backlog is
/// cleared over several sweeps and the enclosure is not held for long. Each removal is
/// an fsynced journal write.
pub const SWEEP_LIMIT: usize = 1000;

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
            let store = enclosure.fabric_mut().store_mut();
            let mut removed = 0;
            for id in &ids {
                match store.remove_object_durable(id) {
                    Ok(true) => removed += 1,
                    Ok(false) => {}
                    Err(e) => {
                        return Err(format!(
                            "the {} sweep stopped after {removed}; the ledger records it: {e}",
                            rule.object_type
                        ))
                    }
                }
            }
            out.push(Expired {
                object_type: rule.object_type.clone(),
                removed,
                more,
            });
        }
        Ok(out)
    }
}
