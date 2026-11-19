//! Cached deliveries and the state file. `[AC-C-2]` `[AC-C-13]` `[FR-C-4]`
//!
//! A delivery of `<collection>` is cached as one sealed object per shown row,
//! `<collection>/<id>` at the row's own marking, and a sealed index,
//! `<collection>#index` at the session's own clearance, listing the shown rows (id,
//! marking, signature status) and the withheld rows **by level only**: a withheld
//! row has no id or body to cache, and none is made up.
//!
//! The state file is the whole cache, with the pending queue ([`PENDING_OBJECT`])
//! and the clock and action counter ([`CLOCK_OBJECT`]) sealed inside it, sealed
//! again by the store
//! ([`LocalStore::export_sealed`](crate::store::LocalStore::export_sealed)) under
//! the device root, bound to the persona, padded, and numbered by a generation the
//! shell keeps outside the file to refuse a rollback. Loading it is all or nothing.
//!
//! The core's own objects start with
//! [`RESERVED_PREFIX`](crate::store::RESERVED_PREFIX) (`'\0'`); a collection name
//! that is empty or holds `'\0'`, `'/'` or `'#'` is refused, so no row or index
//! key can name one, and `edit_field` refuses them too.

use attest::json::{parse, Json};
use policy::{dominates, ClassificationLevel, Marking};
use pqc_agility::Rng;

use crate::demo::audit::SignatureStatus;
use crate::demo::canon;
use crate::keystore::{Keystore, TrustRoot};
use crate::marking::{MarkingLevel, RowMarking};
use crate::source::Delivered;
use crate::store::{StoreError, RESERVED_PREFIX};
use crate::sync::SyncOp;
use crate::{ClientCore, CoreError};

/// The cache object holding the pending queue inside a state file, at the
/// session's own clearance.
pub const PENDING_OBJECT: &str = "\0sys/pending";
/// The cache object holding the Lamport clock and the action counter, at the lowest
/// marking (two counters, nothing secret), so a lower clearance still restores them
/// and no version or action number is ever reused.
pub const CLOCK_OBJECT: &str = "\0sys/clock";
/// The cache object holding the session's device-audit signatures for this persona
/// (`crate::session`), at the session's own clearance. It stays in the cache, so it
/// goes into every state file and is dropped with everything else on a persona
/// switch.
pub const DEVICE_AUDIT_OBJECT: &str = "\0sys/device-audit";

/// A state file to write, and its generation (for the shell to keep outside the
/// file, in a this-device-only keychain item, and pass back on import).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedState {
    /// The sealed file.
    pub file: Vec<u8>,
    /// Its generation.
    pub generation: u64,
}

/// What [`ClientCore::import_state`] loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// Cached rows and indexes loaded.
    pub kept: usize,
    /// Cached rows and indexes dropped (above the current clearance).
    pub dropped: usize,
    /// The pending queue was in the file but the current clearance may not read it,
    /// so the unsent actions are lost: tell the user.
    pub pending_dropped: bool,
    /// The file's generation.
    pub generation: u64,
}

fn bad(what: impl std::fmt::Display) -> CoreError {
    CoreError::Malformed(what.to_string())
}

fn index_key(collection: &str) -> String {
    format!("{collection}#index")
}

fn row_key(collection: &str, id: &str) -> String {
    format!("{collection}/{id}")
}

fn status_word(s: SignatureStatus) -> &'static str {
    s.as_str()
}

fn status_of(word: &str) -> Option<SignatureStatus> {
    [
        SignatureStatus::Verified,
        SignatureStatus::Device,
        SignatureStatus::Unsigned,
    ]
    .into_iter()
    .find(|s| s.as_str() == word)
}

fn strs(v: Option<&Json>) -> Option<Vec<String>> {
    v?.as_array()?
        .iter()
        .map(|x| x.as_str().map(str::to_string))
        .collect()
}

impl<K: Keystore + TrustRoot> ClientCore<K> {
    fn own_marking(&self) -> Marking {
        let c = self.store.clearance();
        Marking {
            level: c.level,
            compartments: c.compartments.clone(),
        }
    }

    /// Cache a delivery of `collection`, replacing the one cached before. Refused,
    /// with nothing changed, if any shown row is above this session's clearance (a
    /// source never delivers one). Withheld rows are recorded by level only.
    /// `[AC-C-13]`
    pub fn cache_delivery(
        &mut self,
        collection: &str,
        rows: &[Delivered],
        rng: &mut dyn Rng,
    ) -> Result<(), CoreError> {
        if collection.is_empty() || collection.contains([RESERVED_PREFIX, '/', '#']) {
            return Err(CoreError::Reserved(collection.escape_debug().to_string()));
        }
        let clearance = self.store.clearance().clone();
        let above = rows.iter().any(|d| match d {
            Delivered::Row { marking, .. } => !dominates(&clearance, &marking.to_policy()),
            Delivered::Withheld { .. } => false,
        });
        if above {
            return Err(StoreError::AboveClearance.into());
        }
        let prefix = row_key(collection, "");
        let stale: Vec<String> = self
            .store
            .visible_ids()
            .into_iter()
            .filter(|k| k.starts_with(&prefix))
            .collect();
        for k in stale {
            self.store.remove(&k);
        }
        let mut index = Vec::with_capacity(rows.len());
        for d in rows {
            match d {
                Delivered::Row {
                    id,
                    marking,
                    body,
                    signature,
                } => {
                    self.store
                        .put(&row_key(collection, id), marking.to_policy(), body, rng)?;
                    let mut entry = vec![
                        ("id".to_string(), Json::Str(id.clone())),
                        (
                            "level".to_string(),
                            Json::Str(marking.level().as_str().into()),
                        ),
                        (
                            "compartments".to_string(),
                            Json::Arr(
                                marking
                                    .compartments()
                                    .iter()
                                    .map(|c| Json::Str(c.clone()))
                                    .collect(),
                            ),
                        ),
                    ];
                    if let Some(s) = signature {
                        entry.push(("sig".to_string(), Json::Str(status_word(*s).into())));
                    }
                    index.push(Json::Obj(entry));
                }
                Delivered::Withheld { level } => index.push(Json::Obj(vec![(
                    "withheld".to_string(),
                    Json::Str(level.as_str().into()),
                )])),
            }
        }
        let own = self.own_marking();
        let text = canon::encode(&Json::Arr(index));
        self.store
            .put(&index_key(collection), own, text.as_bytes(), rng)?;
        Ok(())
    }

    /// The cached delivery of `collection`, exactly as it was delivered. All or
    /// nothing: a missing row or a malformed index is an error, never a partial
    /// list.
    pub fn cached_delivery(&self, collection: &str) -> Result<Vec<Delivered>, CoreError> {
        let index = self.store.get(&index_key(collection))?;
        let text = std::str::from_utf8(&index).map_err(|_| bad("index is not UTF-8"))?;
        let v = parse(text).map_err(|e| bad(format!("index: {e}")))?;
        let entries = v.as_array().ok_or_else(|| bad("index is not an array"))?;
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            if let Some(w) = e.get("withheld") {
                let level = w
                    .as_str()
                    .and_then(MarkingLevel::parse)
                    .ok_or_else(|| bad("withheld level"))?;
                out.push(Delivered::Withheld { level });
                continue;
            }
            let id = e
                .get("id")
                .and_then(Json::as_str)
                .ok_or_else(|| bad("row id"))?;
            let level = e
                .get("level")
                .and_then(Json::as_str)
                .and_then(MarkingLevel::parse)
                .ok_or_else(|| bad("row level"))?;
            let compartments = strs(e.get("compartments")).ok_or_else(|| bad("compartments"))?;
            let signature = match e.get("sig") {
                None => None,
                Some(s) => Some(
                    s.as_str()
                        .and_then(status_of)
                        .ok_or_else(|| bad("signature status"))?,
                ),
            };
            let body = self.store.get(&row_key(collection, id))?;
            out.push(Delivered::Row {
                id: id.to_string(),
                marking: RowMarking::new(level, compartments),
                body,
                signature,
            });
        }
        Ok(out)
    }

    /// The state to write to disk: the sealed cache with the pending queue, the
    /// clock and the action counter sealed inside it, bound to this persona, at a
    /// generation one higher than the last written or loaded. `[FR-C-4]` `[AC-C-2]`
    pub fn export_state(&mut self, rng: &mut dyn Rng) -> Result<ExportedState, CoreError> {
        let mut list = Vec::new();
        for op in self.replica.pending() {
            let b = op.encode();
            list.extend_from_slice(&(b.len() as u32).to_le_bytes());
            list.extend_from_slice(&b);
        }
        let mut clock = self.replica.lamport().to_le_bytes().to_vec();
        clock.extend_from_slice(&self.next_action.to_le_bytes());
        let lowest = Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: Default::default(),
        };
        let own = self.own_marking();
        let file = self
            .store
            .put(PENDING_OBJECT, own, &list, rng)
            .and_then(|()| self.store.put(CLOCK_OBJECT, lowest, &clock, rng))
            .and_then(|()| self.store.export_sealed(rng));
        self.store.remove(PENDING_OBJECT);
        self.store.remove(CLOCK_OBJECT);
        Ok(ExportedState {
            file: file?,
            generation: self.store.generation(),
        })
    }

    /// Load state written by [`export_state`](Self::export_state): the cache
    /// (replacing what is cached; rows above the current clearance dropped), the
    /// pending queue (requeued with its original versions, so a re-push gets the
    /// decision the source already made), and the clock and action counter (raised
    /// to at least the saved values). `min_generation` is the newest generation the
    /// shell has seen; an older file is a rollback ([`CoreError::Stale`]). All or
    /// nothing: on any error the cache is left empty and the queue, clock and
    /// counter as they were; discard the file and refetch. Either way the next
    /// export is numbered above `min_generation`. `[FR-C-4]` `[AC-C-13]`
    pub fn import_state(
        &mut self,
        file: &[u8],
        min_generation: u64,
    ) -> Result<ImportReport, CoreError> {
        self.import(file, min_generation, true)
    }

    /// [`import_state`](Self::import_state) for a session whose source takes no
    /// actions (no data source connected): the pending queue in the file is not
    /// requeued, since no op could ever drain, and the report says it was dropped.
    /// `[FR-C-4]` `[FR-C-5]`
    pub(crate) fn import_state_without_queue(
        &mut self,
        file: &[u8],
        min_generation: u64,
    ) -> Result<ImportReport, CoreError> {
        self.import(file, min_generation, false)
    }

    fn import(
        &mut self,
        file: &[u8],
        min_generation: u64,
        requeue: bool,
    ) -> Result<ImportReport, CoreError> {
        let imported = match self.store.import_sealed(file, min_generation) {
            Ok(i) => i,
            Err(e) => {
                self.store.clear();
                return Err(e.into());
            }
        };
        let parsed = self.pending_in_store().and_then(|ops| {
            let clock = self.clock_in_store()?;
            Ok((ops, clock))
        });
        let (ops, clock) = match parsed {
            Ok(p) => p,
            Err(e) => {
                self.store.clear();
                return Err(e);
            }
        };
        let sys = [PENDING_OBJECT, CLOCK_OBJECT, DEVICE_AUDIT_OBJECT]
            .iter()
            .filter(|k| self.store.contains(k))
            .count();
        self.store.remove(PENDING_OBJECT);
        self.store.remove(CLOCK_OBJECT);
        if let Some((lamport, next_action)) = clock {
            self.replica.observe(lamport);
            self.next_action = self.next_action.max(next_action);
        }
        let unqueued = !requeue && !ops.is_empty();
        if requeue {
            self.replica.requeue(ops);
        }
        let reserved = imported.dropped_reserved.len();
        Ok(ImportReport {
            kept: imported.kept - sys,
            dropped: imported.dropped - reserved,
            pending_dropped: unqueued
                || imported
                    .dropped_reserved
                    .iter()
                    .any(|k| k == PENDING_OBJECT),
            generation: imported.generation,
        })
    }

    /// The session's device-audit record, if one is cached. `[NFR-PQ-3]`
    pub(crate) fn device_audit_record(&self) -> Result<Option<Vec<u8>>, CoreError> {
        match self.store.get(DEVICE_AUDIT_OBJECT) {
            Ok(b) => Ok(Some(b)),
            Err(StoreError::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Seal the session's device-audit record at the session's own clearance.
    pub(crate) fn set_device_audit_record(
        &mut self,
        bytes: &[u8],
        rng: &mut dyn Rng,
    ) -> Result<(), CoreError> {
        let own = self.own_marking();
        Ok(self.store.put(DEVICE_AUDIT_OBJECT, own, bytes, rng)?)
    }

    fn clock_in_store(&self) -> Result<Option<(u64, u64)>, CoreError> {
        let b = match self.store.get(CLOCK_OBJECT) {
            Ok(b) => b,
            Err(StoreError::NotFound) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let b: [u8; 16] = b.try_into().map_err(|_| bad("clock"))?;
        let lamport = u64::from_le_bytes(b[..8].try_into().expect("8 bytes"));
        let next_action = u64::from_le_bytes(b[8..].try_into().expect("8 bytes"));
        Ok(Some((lamport, next_action)))
    }

    fn pending_in_store(&self) -> Result<Vec<SyncOp>, CoreError> {
        let list = match self.store.get(PENDING_OBJECT) {
            Ok(list) => list,
            Err(StoreError::NotFound) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut ops = Vec::new();
        let mut at = 0usize;
        while at < list.len() {
            let n = list
                .get(at..at + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                .ok_or_else(|| bad("pending list"))?;
            let blob = at
                .checked_add(4 + n)
                .and_then(|end| list.get(at + 4..end))
                .ok_or_else(|| bad("pending list"))?;
            ops.push(SyncOp::decode(blob)?);
            at += 4 + n;
        }
        Ok(ops)
    }
}
