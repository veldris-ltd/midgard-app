//! A signed-in session: one persona, one sealed cache, one data source, one device.
//! `[FR-C-1]` `[AC-C-13]` `[FR-C-4]` `[AC-C-2]` `[NFR-PQ-3]`
//!
//! [`Session`] is the only thing the bindings talk to. It composes the core
//! ([`ClientCore`]: the persona-scoped sealed cache and the CRDT queue), a
//! [`DataSource`] and this device's [`DeviceAuditKey`]:
//!
//! - **Reads** ([`Session::rows`]): online, the delivery is cached
//!   ([`ClientCore::cache_delivery`]: refused whole if any shown row is above the
//!   session's clearance; withheld rows kept by level only) and returned; offline,
//!   the cached delivery is returned ([`ClientCore::cached_delivery`], all or
//!   nothing). Either way rows decode into the generated read model.
//! - **Audit status.** A source may say a row is `verified` (the demo enclosure
//!   checked it under the pinned demo-audit anchor) or `none`; a source's `device`
//!   claim is never believed. An audit row is shown as `device` only when this
//!   session holds a device signature for it and the row as delivered verifies
//!   under this device's key, checked on every read, online or offline.
//! - **Device signatures.** When a sync answers one of this persona's actions with
//!   an audit row id, the session fetches the audit collections, signs that row
//!   (only if its actor is this persona and its resource the action's target) and
//!   keeps the signature in the persona's sealed cache
//!   ([`DEVICE_AUDIT_OBJECT`](crate::DEVICE_AUDIT_OBJECT)). A row not found yet is
//!   signed when it is next fetched; a row signed once is never signed again.
//! - **Actions** ([`Session::request_action`]): four `edit_field` calls on a new
//!   action object ([`ClientCore::new_action_id`]), at the target row's marking, so
//!   acting on a row above clearance is refused before anything is queued.
//! - **Sync** ([`Session::sync`]): the pending ops are pushed (encoded, not
//!   drained), the answer is merged (all or nothing) and only then is the queue
//!   drained, so a failed push or a malformed answer loses nothing.
//! - **Persona switch** ([`Session::switch_persona`]): refused while ops are
//!   pending; otherwise the core is re-scoped and nothing of the previous persona
//!   is kept (cache, CRDT view, device signatures).
//! - **State file** ([`Session::export_state`], [`Session::import_state`]): the
//!   shell keeps the generation outside the file, per persona, and passes it back.
//!   On any import error the cache is empty: discard the file; the next read
//!   refetches.

use std::collections::BTreeMap;
use std::fmt;

use attest::json::{parse, Json};
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

use crate::consoles::decode::{DecodeError, FromJson};
use crate::demo::audit::{self, SignatureStatus};
use crate::demo::canon;
use crate::device_audit::{DeviceAuditKey, DeviceKeyError};
use crate::keystore::{Keystore, SecurityFloor, TrustRoot};
use crate::marking::{RowMarking, Visible};
use crate::source::{action, decode_rows, DataSource, Delivered, Grant, SourceError};
use crate::store::StoreError;
use crate::sync::SyncOp;
use crate::{ClientCore, CoreError, ExportedState, ImportReport};

/// Coarse sync phase (the words `ClientSyncPhase` uses in sdk/ts/src/products/clients.ts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPhase {
    /// Nothing pending.
    Synced,
    /// Edits are queued and the source is reachable.
    Syncing,
    /// The source is unreachable; edits queue.
    Offline,
    /// The last sync returned a conflict.
    Conflict,
}

/// Sync readout for the chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStatus {
    /// Phase.
    pub phase: SyncPhase,
    /// Ops queued locally.
    pub pending: usize,
    /// Unix seconds of the last successful sync in this session.
    pub last_sync: Option<u64>,
}

/// What a sync did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Ops pushed.
    pub pushed: usize,
    /// Ops merged back.
    pub merged: usize,
}

/// An action's state as this device knows it.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionState {
    /// Queued or pushed, not yet answered.
    Queued,
    /// Applied by the source.
    Applied {
        /// Audit record id.
        audit: String,
        /// Canonical JSON result.
        result: String,
    },
    /// Refused by the source.
    Refused {
        /// Why.
        reason: String,
    },
    /// The target changed under it.
    Conflict {
        /// Why.
        reason: String,
    },
    /// No such action on this device (or not in this persona's view).
    Unknown,
}

/// A session error, each kind a screen state.
#[derive(Debug)]
pub enum SessionError {
    /// The row or action is above the session's clearance (or a source delivered a
    /// row above it, which is refused whole).
    AboveClearance,
    /// The source is unreachable.
    Offline,
    /// Offline and nothing cached for this collection.
    NothingCached(String),
    /// A persona switch with unsent work: sync first.
    PendingOps(usize),
    /// The source refused.
    Source(SourceError),
    /// The core refused (custody, tamper, rollback, malformed op or state).
    Core(CoreError),
    /// A row did not decode into its read model.
    Decode(DecodeError),
    /// The device-audit key could not sign.
    Device(DeviceKeyError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::AboveClearance => write!(f, "above clearance"),
            SessionError::Offline => write!(f, "offline"),
            SessionError::NothingCached(c) => write!(f, "offline and nothing cached for {c}"),
            SessionError::PendingOps(n) => write!(f, "{n} unsent op(s); sync first"),
            SessionError::Source(e) => write!(f, "{e}"),
            SessionError::Core(e) => write!(f, "{e}"),
            SessionError::Decode(e) => write!(f, "{}: {}", e.path, e.reason),
            SessionError::Device(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<CoreError> for SessionError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Store(StoreError::AboveClearance) => SessionError::AboveClearance,
            CoreError::PendingOps(n) => SessionError::PendingOps(n),
            e => SessionError::Core(e),
        }
    }
}

impl From<SourceError> for SessionError {
    fn from(e: SourceError) -> Self {
        match e {
            SourceError::Offline => SessionError::Offline,
            e => SessionError::Source(e),
        }
    }
}

impl From<DecodeError> for SessionError {
    fn from(e: DecodeError) -> Self {
        SessionError::Decode(e)
    }
}

impl From<DeviceKeyError> for SessionError {
    fn from(e: DeviceKeyError) -> Self {
        SessionError::Device(e)
    }
}

/// This persona's device-audit record: signatures by `<collection>/<id>`, and the
/// audit row ids owed a signature (answered, not yet seen), with the action's target.
#[derive(Debug, Default, Clone, PartialEq)]
struct DeviceRecord {
    sigs: BTreeMap<String, String>,
    owed: BTreeMap<String, String>,
}

impl DeviceRecord {
    fn encode(&self) -> Vec<u8> {
        let map = |m: &BTreeMap<String, String>| {
            Json::Obj(
                m.iter()
                    .map(|(k, v)| (k.clone(), Json::Str(v.clone())))
                    .collect(),
            )
        };
        canon::encode(&Json::Obj(vec![
            ("owed".into(), map(&self.owed)),
            ("sigs".into(), map(&self.sigs)),
        ]))
        .into_bytes()
    }

    /// Strict; `None` on anything unexpected.
    fn decode(bytes: &[u8]) -> Option<Self> {
        let v = parse(std::str::from_utf8(bytes).ok()?).ok()?;
        let Json::Obj(top) = &v else { return None };
        if top.len() != 2 {
            return None;
        }
        let map = |k: &str| -> Option<BTreeMap<String, String>> {
            let Json::Obj(pairs) = v.get(k)? else {
                return None;
            };
            pairs
                .iter()
                .map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        };
        Some(Self {
            sigs: map("sigs")?,
            owed: map("owed")?,
        })
    }

    /// Whether a signature is held for audit row `id` in any collection (a
    /// collection name never holds `/`, so the first one ends it).
    fn signed(&self, id: &str) -> bool {
        self.sigs
            .keys()
            .any(|k| k.split_once('/').is_some_and(|(_, i)| i == id))
    }
}

/// An audit row's body with its `pqSignature` set to `status`'s word; `None` if the
/// body is not a JSON object.
fn restamp(body: &[u8], status: SignatureStatus) -> Option<(Json, Vec<u8>)> {
    let item = parse(std::str::from_utf8(body).ok()?).ok()?;
    if !matches!(item, Json::Obj(_)) {
        return None;
    }
    let item = audit::with_status(&item, status);
    let bytes = canon::encode(&item).into_bytes();
    Some((item, bytes))
}

/// A delivery as the session caches it: a source's `device` claim becomes `none`,
/// and every audit row's `pqSignature` text says what its status says.
fn normalise(rows: Vec<Delivered>) -> Vec<Delivered> {
    rows.into_iter()
        .map(|d| match d {
            Delivered::Row {
                id,
                marking,
                body,
                signature: Some(s),
            } => {
                let s = match s {
                    SignatureStatus::Device => SignatureStatus::Unsigned,
                    s => s,
                };
                let body = restamp(&body, s).map_or(body, |(_, b)| b);
                Delivered::Row {
                    id,
                    marking,
                    body,
                    signature: Some(s),
                }
            }
            d => d,
        })
        .collect()
}

fn row_key(collection: &str, id: &str) -> String {
    format!("{collection}/{id}")
}

/// A signed-in session.
pub struct Session<K: Keystore + TrustRoot, S: DataSource> {
    core: ClientCore<K>,
    source: S,
    grant: Grant,
    device: DeviceAuditKey,
    record: DeviceRecord,
    last_sync: Option<u64>,
    conflict: bool,
}

impl<K: Keystore + TrustRoot, S: DataSource> Session<K, S> {
    /// Sign `persona_id` in over `keystore` (the device root), `device` (this
    /// device's audit key, opened under the same root) and `source`. The cache is
    /// empty; load the persona's state file with [`import_state`](Self::import_state).
    pub fn open(
        keystore: K,
        device: DeviceAuditKey,
        source: S,
        persona_id: &str,
        replica_id: &str,
    ) -> Result<Self, SessionError> {
        let grant = source.grant(persona_id)?;
        let core = ClientCore::open_scoped(
            provider(),
            keystore,
            grant.clearance().clone(),
            SecurityFloor::NonExtractable,
            replica_id,
            persona_id,
        )?;
        Ok(Self {
            core,
            source,
            grant,
            device,
            record: DeviceRecord::default(),
            last_sync: None,
            conflict: false,
        })
    }

    /// The grant (persona, clearance).
    pub fn grant(&self) -> &Grant {
        &self.grant
    }

    /// The data source.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// The device-audit key id (a digest of its verifying key).
    pub fn device_key_id(&self) -> &str {
        self.device.key_id()
    }

    /// Simulate (demo) going offline or back online.
    pub fn set_online(&mut self, online: bool) {
        self.source.set_online(online);
    }

    /// The rows of `collection`, decoded into `T`: shown rows in full, rows above
    /// the session's clearance by level only.
    pub fn rows<T: FromJson>(&mut self, collection: &str) -> Result<Vec<Visible<T>>, SessionError> {
        let delivered = match self.source.fetch(&self.grant, collection) {
            Ok(d) => {
                let d = normalise(d);
                self.core.cache_delivery(collection, &d, &mut OsRng)?;
                if self.sign_owed(collection, &d)? {
                    self.save_record()?;
                }
                d
            }
            Err(SourceError::Offline) => self.cached(collection)?,
            Err(e) => return Err(e.into()),
        };
        Ok(decode_rows(self.device_checked(collection, delivered))?)
    }

    fn cached(&self, collection: &str) -> Result<Vec<Delivered>, SessionError> {
        match self.core.cached_delivery(collection) {
            Ok(d) => Ok(normalise(d)),
            Err(CoreError::Store(StoreError::NotFound)) => {
                Err(SessionError::NothingCached(collection.to_string()))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// `rows` with each unchecked audit row this session holds a signature for
    /// checked under the device key: `device` if it verifies, `none` otherwise.
    fn device_checked(&self, collection: &str, rows: Vec<Delivered>) -> Vec<Delivered> {
        rows.into_iter()
            .map(|d| match d {
                Delivered::Row {
                    id,
                    marking,
                    body,
                    signature: Some(SignatureStatus::Unsigned),
                } => {
                    let checked = self
                        .record
                        .sigs
                        .get(&row_key(collection, &id))
                        .and_then(|sig| {
                            let (item, _) = restamp(&body, SignatureStatus::Unsigned)?;
                            self.device
                                .verify_row(collection, &id, &marking, &item, sig)
                                .ok()?;
                            restamp(&body, SignatureStatus::Device).map(|(_, b)| b)
                        });
                    match checked {
                        Some(body) => Delivered::Row {
                            id,
                            marking,
                            body,
                            signature: Some(SignatureStatus::Device),
                        },
                        None => Delivered::Row {
                            id,
                            marking,
                            body,
                            signature: Some(SignatureStatus::Unsigned),
                        },
                    }
                }
                d => d,
            })
            .collect()
    }

    /// Sign each owed audit row of `rows` whose actor is this persona and whose
    /// resource is the action's target. Whether the record changed.
    fn sign_owed(&mut self, collection: &str, rows: &[Delivered]) -> Result<bool, SessionError> {
        let mut changed = false;
        for d in rows {
            let Delivered::Row {
                id,
                marking,
                body,
                signature: Some(SignatureStatus::Unsigned),
            } = d
            else {
                continue;
            };
            let Some(target) = self.record.owed.get(id).cloned() else {
                continue;
            };
            self.record.owed.remove(id);
            changed = true;
            let Some((item, _)) = restamp(body, SignatureStatus::Unsigned) else {
                continue;
            };
            let field = |k: &str| item.get(k).and_then(Json::as_str);
            if field("actor") != Some(self.grant.persona().id.as_str())
                || field("resource") != Some(target.as_str())
            {
                continue;
            }
            let sig = self.device.sign_row(collection, id, marking, &item)?;
            self.record.sigs.insert(row_key(collection, id), sig);
        }
        Ok(changed)
    }

    fn save_record(&mut self) -> Result<(), SessionError> {
        let bytes = self.record.encode();
        Ok(self.core.set_device_audit_record(&bytes, &mut OsRng)?)
    }

    /// Reload the record from the cache. A record that does not open or parse is
    /// dropped: its rows then show `none`, which is true of an unchecked row.
    fn load_record(&mut self) {
        self.record = self
            .core
            .device_audit_record()
            .ok()
            .flatten()
            .and_then(|b| DeviceRecord::decode(&b))
            .unwrap_or_default();
    }

    /// Queue an action on `target` (a `<collection>/<id>` whose row carries
    /// `marking`). Refused, with nothing queued, above the session's clearance.
    pub fn request_action(
        &mut self,
        verb: &str,
        target: &str,
        params: &[(String, String)],
        marking: &RowMarking,
    ) -> Result<String, SessionError> {
        if !crate::marking::may_see(self.grant.clearance(), marking) {
            return Err(SessionError::AboveClearance);
        }
        let id = self.core.new_action_id(&mut OsRng);
        let params = canon::encode(&Json::Obj(
            params
                .iter()
                .map(|(k, v)| (k.clone(), Json::Str(v.clone())))
                .collect(),
        ));
        let m = marking.to_policy();
        let by = self.grant.persona().id.clone();
        for (field, value) in [
            (action::VERB, verb),
            (action::TARGET, target),
            (action::PARAMS, params.as_str()),
            (action::BY, by.as_str()),
        ] {
            self.core
                .edit_field(&id, field, value.as_bytes().to_vec(), m.clone(), &mut OsRng)?;
        }
        Ok(id)
    }

    /// What this device knows about action `id`.
    pub fn action_state(&self, id: &str) -> ActionState {
        let field = |f: &str| {
            self.core
                .read_field(id, f)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        match field(action::STATUS).as_deref() {
            None if field(action::VERB).is_some() => ActionState::Queued,
            None => ActionState::Unknown,
            Some("applied") => ActionState::Applied {
                audit: field(action::AUDIT).unwrap_or_default(),
                result: field(action::RESULT).unwrap_or_default(),
            },
            Some("conflict") => ActionState::Conflict {
                reason: field(action::REASON).unwrap_or_default(),
            },
            Some(_) => ActionState::Refused {
                reason: field(action::REASON).unwrap_or_default(),
            },
        }
    }

    /// Push queued ops and merge the answer, then sign the audit rows made for this
    /// persona's actions. On a failed push or a malformed answer nothing is lost.
    pub fn sync(&mut self) -> Result<SyncReport, SessionError> {
        let blobs: Vec<Vec<u8>> = self.core.pending_ops().iter().map(SyncOp::encode).collect();
        let back = self.source.exchange(&self.grant, &blobs)?;
        let answer: Vec<SyncOp> = back
            .iter()
            .map(|b| SyncOp::decode(b))
            .collect::<Result<_, _>>()
            .map_err(CoreError::from)?;
        self.core.merge_remote(&answer);
        self.core.drain_pending();
        self.conflict = answer
            .iter()
            .any(|op| op.field == action::STATUS && op.value.as_deref() == Some(b"conflict"));
        self.last_sync = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs());

        // The audit rows answered for this persona's own actions are owed a
        // signature (once: a re-sent answer for a row signed already adds nothing).
        let me = self.grant.persona().id.clone();
        let mut changed = false;
        for op in answer
            .iter()
            .filter(|op| op.field == action::AUDIT && op.object.starts_with(action::PREFIX))
        {
            let by = self.core.read_field(&op.object, action::BY);
            let target = self.core.read_field(&op.object, action::TARGET);
            let (Some(audit_id), Some(target)) = (
                op.value
                    .as_deref()
                    .and_then(|v| std::str::from_utf8(v).ok()),
                target.and_then(|t| String::from_utf8(t).ok()),
            ) else {
                continue;
            };
            if by.as_deref() != Some(me.as_bytes())
                || audit_id.is_empty()
                || self.record.signed(audit_id)
                || self.record.owed.contains_key(audit_id)
            {
                continue;
            }
            self.record.owed.insert(audit_id.to_string(), target);
            changed = true;
        }
        if !self.record.owed.is_empty() {
            let audit_collections: Vec<String> = self
                .source
                .collections()
                .into_iter()
                .filter(|(_, t)| t == audit::AUDIT_TYPE)
                .map(|(n, _)| n)
                .collect();
            for c in audit_collections {
                if self.record.owed.is_empty() {
                    break;
                }
                // Best effort: a row not fetched now is signed when next read.
                if let Ok(rows) = self.source.fetch(&self.grant, &c) {
                    changed |= self.sign_owed(&c, &normalise(rows))?;
                }
            }
        }
        if changed {
            self.save_record()?;
        }
        Ok(SyncReport {
            pushed: blobs.len(),
            merged: back.len(),
        })
    }

    /// The sync chip.
    pub fn sync_status(&self) -> SyncStatus {
        let pending = self.core.pending_ops().len();
        let phase = if !self.source.online() {
            SyncPhase::Offline
        } else if self.conflict {
            SyncPhase::Conflict
        } else if pending > 0 {
            SyncPhase::Syncing
        } else {
            SyncPhase::Synced
        };
        SyncStatus {
            phase,
            pending,
            last_sync: self.last_sync,
        }
    }

    /// Switch persona. Refused while ops are pending ([`SessionError::PendingOps`]:
    /// sync first), and for a persona the source does not grant; either way the
    /// session comes back unchanged. Otherwise nothing of the previous persona is
    /// kept: cache, CRDT view and device signatures are dropped. Export the
    /// previous persona's state first if it is to be kept. `[AC-C-13]`
    pub fn switch_persona(self, persona_id: &str) -> Result<Self, (Self, SessionError)> {
        let pending = self.core.pending_ops().len();
        if pending > 0 {
            return Err((self, SessionError::PendingOps(pending)));
        }
        let grant = match self.source.grant(persona_id) {
            Ok(g) => g,
            Err(e) => return Err((self, e.into())),
        };
        let Self {
            core,
            source,
            grant: old_grant,
            device,
            record,
            last_sync,
            conflict,
        } = self;
        match core.rescope(persona_id, grant.clearance().clone()) {
            Ok((core, _dropped)) => {
                let mut s = Self {
                    core,
                    source,
                    grant,
                    device,
                    record: DeviceRecord::default(),
                    last_sync: None,
                    conflict: false,
                };
                // Re-scoping to the same persona keeps what it still dominates.
                s.load_record();
                Ok(s)
            }
            Err((core, e)) => Err((
                Self {
                    core,
                    source,
                    grant: old_grant,
                    device,
                    record,
                    last_sync,
                    conflict,
                },
                e.into(),
            )),
        }
    }

    /// End the session, keeping the source (its demo state outlives a lock). The
    /// keystore and the device key, and with them the unlocked secrets, are
    /// dropped and zeroised.
    pub fn into_source(self) -> S {
        self.source
    }

    /// The sealed state to write to disk, and its generation (for the shell to keep
    /// outside the file, per persona, after the file is written).
    pub fn export_state(&mut self) -> Result<ExportedState, SessionError> {
        Ok(self.core.export_state(&mut OsRng)?)
    }

    /// Load this persona's state file. `min_generation` is the newest generation the
    /// shell has kept for this persona. [`ImportReport::pending_dropped`] says the
    /// unsent actions in the file were lost (tell the user). On any error the cache
    /// is empty and the queue unchanged: discard the file; the next read refetches.
    pub fn import_state(
        &mut self,
        file: &[u8],
        min_generation: u64,
    ) -> Result<ImportReport, SessionError> {
        match self.core.import_state(file, min_generation) {
            Ok(report) => {
                self.load_record();
                Ok(report)
            }
            Err(e) => {
                self.record = DeviceRecord::default();
                Err(e.into())
            }
        }
    }
}
