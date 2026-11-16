//! Cached deliveries and the state file. `[AC-C-2]` `[AC-C-13]` `[FR-C-4]`
//!
//! A delivery of `<collection>` is cached as one sealed object per shown row,
//! `<collection>/<id>` at the row's own marking, and a sealed index,
//! `<collection>#index` at the session's own clearance, listing the shown rows (id,
//! marking, signature status) and the withheld rows **by level only**: a withheld
//! row has no id or body to cache, and none is made up.
//!
//! The state file is the whole cache, with the pending queue sealed inside it as
//! [`PENDING_OBJECT`], sealed again by the store
//! ([`LocalStore::export_sealed`](crate::store::LocalStore::export_sealed)) under
//! the device root and bound to the persona. Loading it is all or nothing.

use attest::json::{parse, Json};
use policy::{dominates, Marking};
use pqc_agility::Rng;

use crate::demo::audit::SignatureStatus;
use crate::demo::canon;
use crate::keystore::{Keystore, TrustRoot};
use crate::marking::{MarkingLevel, RowMarking};
use crate::source::Delivered;
use crate::store::StoreError;
use crate::sync::SyncOp;
use crate::{ClientCore, CoreError};

/// The cache object holding the pending queue inside a state file.
pub const PENDING_OBJECT: &str = "sys/pending";

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

    /// The state to write to disk: the sealed cache with the pending queue sealed
    /// inside it, bound to this persona. `[FR-C-4]` `[AC-C-2]`
    pub fn export_state(&mut self, rng: &mut dyn Rng) -> Result<Vec<u8>, CoreError> {
        let mut list = Vec::new();
        for op in self.replica.pending() {
            let b = op.encode();
            list.extend_from_slice(&(b.len() as u32).to_le_bytes());
            list.extend_from_slice(&b);
        }
        let own = self.own_marking();
        self.store.put(PENDING_OBJECT, own, &list, rng)?;
        let file = self.store.export_sealed(rng);
        self.store.remove(PENDING_OBJECT);
        Ok(file?)
    }

    /// Load state written by [`export_state`](Self::export_state): the cache
    /// (replacing what is cached; rows above the current clearance dropped) and the
    /// pending queue (requeued with its original versions, so a re-push gets the
    /// decision the source already made). All or nothing: on any error the cache is
    /// left empty and the queue as it was; discard the file and refetch. Returns
    /// `(kept, dropped)` cached objects. `[FR-C-4]` `[AC-C-13]`
    pub fn import_state(&mut self, file: &[u8]) -> Result<(usize, usize), CoreError> {
        let (kept, dropped) = match self.store.import_sealed(file) {
            Ok(counts) => counts,
            Err(e) => {
                self.store.clear();
                return Err(e.into());
            }
        };
        let ops = match self.pending_in_store() {
            Ok(ops) => ops,
            Err(e) => {
                self.store.clear();
                return Err(e);
            }
        };
        let had_pending = self.store.contains(PENDING_OBJECT);
        self.store.remove(PENDING_OBJECT);
        self.replica.requeue(ops);
        Ok((kept - usize::from(had_pending), dropped))
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
