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
//! - **Audit status.** Only rows of a collection the source serves as `AuditRecord`
//!   carry a status; every other row carries none. `verified` is believed only from
//!   a source that checked it inside the core (the demo enclosure, under the pinned
//!   demo-audit anchor: [`DataSource::in_core_audit_check`]); a source's `device`
//!   claim is never believed. An audit row is shown as `device` only when this
//!   session holds a device signature for it, the row is still the one owed for the
//!   recorded action, and the statement verifies under this device's key, checked
//!   on every read, online or offline.
//! - **Device signatures.** [`Session::request_action`] records the action (id,
//!   verb, target, SHA-256 of the parameters) in the persona's sealed record
//!   ([`DEVICE_AUDIT_OBJECT`](crate::DEVICE_AUDIT_OBJECT)) before queueing it. When a
//!   sync answers a recorded action, the merged status and audit row id are owed a
//!   signature; ops about any other object are ignored, and verb and target always
//!   come from the record, never from the answer. The row is signed only if its
//!   actor is this persona, its resource the recorded target and its `action` text
//!   exactly `"<status> · <verb>"`; what is signed is a device statement (the row
//!   plus the recorded action and the status, [`crate::device_audit`]), not the
//!   bare row. A row not found yet is signed when it is next read; a row signed once
//!   is never signed again.
//! - **Actions** ([`Session::request_action`]): the target's marking comes from the
//!   persona's cached delivery, never from the caller, so acting on a row that is
//!   withheld, missing or never read is refused before anything is queued; then four
//!   `edit_field` calls on a new action object ([`ClientCore::new_action_id`]).
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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use attest::hex::hex;
use attest::json::{parse, Json};
use pqc_agility::backend::fips::provider;
use pqc_agility::{sha256, OsRng};

use crate::consoles::decode::{DecodeError, FromJson};
use crate::demo::audit::{self, SignatureStatus};
use crate::demo::canon;
use crate::device_audit::{DeviceAuditKey, DeviceKeyError, DeviceStatement};
use crate::keystore::{Keystore, SecurityFloor, TrustRoot};
use crate::marking::{may_see, Visible};
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
    /// An action target that is not `<collection>/<row id>`.
    BadTarget(String),
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
            SessionError::BadTarget(t) => write!(f, "action target {t:?} is not <collection>/<id>"),
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

/// What this device recorded when it made an action: the source never supplies it.
#[derive(Debug, Clone, PartialEq)]
struct ActionRecord {
    verb: String,
    target: String,
    params_sha256: String,
}

/// An audit row id answered for one of this device's actions, not yet signed.
#[derive(Debug, Clone, PartialEq)]
struct Owed {
    action: String,
    status: String,
}

/// A device signature over a statement about one row.
#[derive(Debug, Clone, PartialEq)]
struct Signed {
    action: String,
    status: String,
    sig: String,
}

/// This persona's device-audit record, sealed in its cache: the actions this device
/// made, the audit rows owed a signature (by audit row id: the source decides which
/// collection holds the row, so the collection is known only when the row is seen),
/// and the signatures (by `<collection>/<id>`).
#[derive(Debug, Default, Clone, PartialEq)]
struct DeviceRecord {
    actions: BTreeMap<String, ActionRecord>,
    owed: BTreeMap<String, Owed>,
    sigs: BTreeMap<String, Signed>,
}

fn obj(fields: &[(&str, &str)]) -> Json {
    Json::Obj(
        fields
            .iter()
            .map(|(k, v)| (k.to_string(), Json::Str(v.to_string())))
            .collect(),
    )
}

/// The string fields `keys` of `v`, which must have exactly those keys.
fn fields<const N: usize>(v: &Json, keys: [&str; N]) -> Option<[String; N]> {
    let Json::Obj(pairs) = v else { return None };
    if pairs.len() != N {
        return None;
    }
    let mut out: [String; N] = std::array::from_fn(|_| String::new());
    for (i, k) in keys.iter().enumerate() {
        out[i] = v.get(k)?.as_str()?.to_string();
    }
    Some(out)
}

impl DeviceRecord {
    fn encode(&self) -> Vec<u8> {
        let map = |m: Vec<(String, Json)>| Json::Obj(m);
        canon::encode(&Json::Obj(vec![
            (
                "actions".into(),
                map(self
                    .actions
                    .iter()
                    .map(|(k, a)| {
                        let v = obj(&[
                            ("paramsSha256", &a.params_sha256),
                            ("target", &a.target),
                            ("verb", &a.verb),
                        ]);
                        (k.clone(), v)
                    })
                    .collect()),
            ),
            (
                "owed".into(),
                map(self
                    .owed
                    .iter()
                    .map(|(k, o)| {
                        (
                            k.clone(),
                            obj(&[("action", &o.action), ("status", &o.status)]),
                        )
                    })
                    .collect()),
            ),
            (
                "sigs".into(),
                map(self
                    .sigs
                    .iter()
                    .map(|(k, s)| {
                        let v = obj(&[
                            ("action", &s.action),
                            ("sig", &s.sig),
                            ("status", &s.status),
                        ]);
                        (k.clone(), v)
                    })
                    .collect()),
            ),
        ]))
        .into_bytes()
    }

    /// Strict; `None` on anything unexpected.
    fn decode(bytes: &[u8]) -> Option<Self> {
        let v = parse(std::str::from_utf8(bytes).ok()?).ok()?;
        let Json::Obj(top) = &v else { return None };
        if top.len() != 3 {
            return None;
        }
        let entries = |k: &str| -> Option<&[(String, Json)]> {
            match v.get(k)? {
                Json::Obj(pairs) => Some(pairs.as_slice()),
                _ => None,
            }
        };
        let mut r = Self::default();
        for (k, e) in entries("actions")? {
            let [params_sha256, target, verb] = fields(e, ["paramsSha256", "target", "verb"])?;
            r.actions.insert(
                k.clone(),
                ActionRecord {
                    verb,
                    target,
                    params_sha256,
                },
            );
        }
        for (k, e) in entries("owed")? {
            let [action, status] = fields(e, ["action", "status"])?;
            r.owed.insert(k.clone(), Owed { action, status });
        }
        for (k, e) in entries("sigs")? {
            let [action, sig, status] = fields(e, ["action", "sig", "status"])?;
            r.sigs.insert(
                k.clone(),
                Signed {
                    action,
                    status,
                    sig,
                },
            );
        }
        Some(r)
    }

    /// Whether a signature is held for audit row `id` in any collection (a
    /// collection name never holds `/`, so the first one ends it).
    fn signed(&self, id: &str) -> bool {
        self.sigs
            .keys()
            .any(|k| k.split_once('/').is_some_and(|(_, i)| i == id))
    }

    /// The statement about `action` answered with `status`, from this record only.
    fn statement(&self, action: &str, status: &str) -> Option<(DeviceStatement, &ActionRecord)> {
        let a = self.actions.get(action)?;
        let st = DeviceStatement {
            action_id: action.to_string(),
            verb: a.verb.clone(),
            target: a.target.clone(),
            params_sha256: a.params_sha256.clone(),
            status: status.to_string(),
        };
        Some((st, a))
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

/// A delivery as the session uses it. Rows of a collection that is not an audit
/// collection carry no signature status at all. In an audit collection a source's
/// `device` claim becomes `none`, so does `verified` from a source outside the core,
/// and every row's `pqSignature` text says what its status says.
fn normalise(rows: Vec<Delivered>, audit_collection: bool, in_core: bool) -> Vec<Delivered> {
    rows.into_iter()
        .map(|d| match d {
            Delivered::Row {
                id, marking, body, ..
            } if !audit_collection => Delivered::Row {
                id,
                marking,
                body,
                signature: None,
            },
            Delivered::Row {
                id,
                marking,
                body,
                signature,
            } => {
                let s = match signature {
                    Some(SignatureStatus::Verified) if in_core => SignatureStatus::Verified,
                    _ => SignatureStatus::Unsigned,
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

/// Whether `item` is the audit row the source owes for `action` answered with
/// `status`, by `me`: actor, resource and action text exactly (no "claimed by").
fn row_is_for(item: &Json, me: &str, action: &ActionRecord, status: &str) -> bool {
    let field = |k: &str| item.get(k).and_then(Json::as_str);
    field("actor") == Some(me)
        && field("resource") == Some(action.target.as_str())
        && field("action") == Some(format!("{status} · {}", action.verb).as_str())
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

    /// Whether the source serves `collection` as audit rows.
    fn is_audit(&self, collection: &str) -> bool {
        self.source
            .collections()
            .iter()
            .any(|(n, t)| n == collection && t == audit::AUDIT_TYPE)
    }

    fn normalised(&self, collection: &str, rows: Vec<Delivered>) -> Vec<Delivered> {
        normalise(
            rows,
            self.is_audit(collection),
            self.source.in_core_audit_check().is_some(),
        )
    }

    /// The rows of `collection`, decoded into `T`: shown rows in full, rows above
    /// the session's clearance by level only.
    pub fn rows<T: FromJson>(&mut self, collection: &str) -> Result<Vec<Visible<T>>, SessionError> {
        let delivered = match self.source.fetch(&self.grant, collection) {
            Ok(d) => {
                let d = self.normalised(collection, d);
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
            Ok(d) => Ok(self.normalised(collection, d)),
            Err(CoreError::Store(StoreError::NotFound)) => {
                Err(SessionError::NothingCached(collection.to_string()))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// `rows` (normalised) with each unchecked row of an audit collection that this
    /// session holds a signature for checked again: the row is still the one owed
    /// for the recorded action, and the statement verifies under the device key.
    /// `device` if both hold, `none` otherwise.
    fn device_checked(&self, collection: &str, rows: Vec<Delivered>) -> Vec<Delivered> {
        if !self.is_audit(collection) {
            return rows;
        }
        let me = self.grant.persona().id.as_str();
        rows.into_iter()
            .map(|d| match d {
                Delivered::Row {
                    id,
                    marking,
                    body,
                    signature: Some(SignatureStatus::Unsigned),
                } => {
                    let checked =
                        self.record
                            .sigs
                            .get(&row_key(collection, &id))
                            .and_then(|signed| {
                                let (st, rec) =
                                    self.record.statement(&signed.action, &signed.status)?;
                                let (item, _) = restamp(&body, SignatureStatus::Unsigned)?;
                                if !row_is_for(&item, me, rec, &signed.status) {
                                    return None;
                                }
                                self.device
                                    .verify_statement(
                                        collection,
                                        &id,
                                        &marking,
                                        &item,
                                        &st,
                                        &signed.sig,
                                    )
                                    .ok()?;
                                restamp(&body, SignatureStatus::Device).map(|(_, b)| b)
                            });
                    let (body, s) = match checked {
                        Some(b) => (b, SignatureStatus::Device),
                        None => (body, SignatureStatus::Unsigned),
                    };
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

    /// Sign a statement about each owed row of `rows` (normalised, of an audit
    /// collection) that is the row owed for the recorded action. A row that is not
    /// is never signed. Whether the record changed.
    fn sign_owed(&mut self, collection: &str, rows: &[Delivered]) -> Result<bool, SessionError> {
        if self.record.owed.is_empty() || !self.is_audit(collection) {
            return Ok(false);
        }
        let me = self.grant.persona().id.clone();
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
            let Some(owed) = self.record.owed.remove(id) else {
                continue;
            };
            changed = true;
            let Some((st, rec)) = self.record.statement(&owed.action, &owed.status) else {
                continue;
            };
            let Some((item, _)) = restamp(body, SignatureStatus::Unsigned) else {
                continue;
            };
            if !row_is_for(&item, &me, rec, &owed.status) {
                continue;
            }
            let sig = self
                .device
                .sign_statement(collection, id, marking, &item, &st)?;
            self.record.sigs.insert(
                row_key(collection, id),
                Signed {
                    action: owed.action,
                    status: owed.status,
                    sig,
                },
            );
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

    /// Queue an action on `target` (`<collection>/<row id>`). The target's marking
    /// is the one in this persona's cached delivery of the collection, never the
    /// caller's: [`SessionError::NothingCached`] if the collection was never read,
    /// [`SessionError::AboveClearance`] if the row is withheld or not there (the
    /// same answer, so it does not say which). Nothing is queued on a refusal.
    pub fn request_action(
        &mut self,
        verb: &str,
        target: &str,
        params: &[(String, String)],
    ) -> Result<String, SessionError> {
        let (collection, row) = target
            .split_once('/')
            .filter(|(c, r)| !c.is_empty() && !r.is_empty())
            .ok_or_else(|| SessionError::BadTarget(target.to_string()))?;
        let cached = match self.core.cached_delivery(collection) {
            Ok(d) => d,
            Err(CoreError::Store(StoreError::NotFound)) => {
                return Err(SessionError::NothingCached(collection.to_string()))
            }
            Err(e) => return Err(e.into()),
        };
        let marking = cached
            .into_iter()
            .find_map(|d| match d {
                Delivered::Row { id, marking, .. } if id == row => Some(marking),
                _ => None,
            })
            .ok_or(SessionError::AboveClearance)?;
        if !may_see(self.grant.clearance(), &marking) {
            return Err(SessionError::AboveClearance);
        }
        let id = self.core.new_action_id(&mut OsRng);
        let params = canon::encode(&Json::Obj(
            params
                .iter()
                .map(|(k, v)| (k.clone(), Json::Str(v.clone())))
                .collect(),
        ));
        // Recorded (sealed) before the action is queued: the device signs only
        // statements about actions in this record.
        self.record.actions.insert(
            id.clone(),
            ActionRecord {
                verb: verb.to_string(),
                target: target.to_string(),
                params_sha256: hex(&sha256(params.as_bytes())),
            },
        );
        self.save_record()?;
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

    /// Push queued ops and merge the answer, then sign statements about the audit
    /// rows made for this device's own actions. On a failed push or a malformed
    /// answer nothing is lost.
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

        // Only actions this device recorded count; for each, the merged status and
        // audit id are owed a signature (once: a re-sent answer for a row signed
        // already adds nothing). Verb and target come from the record, never the
        // answer.
        let answered: BTreeSet<&str> = answer
            .iter()
            .map(|op| op.object.as_str())
            .filter(|o| self.record.actions.contains_key(*o))
            .collect();
        let mut changed = false;
        for action_id in answered {
            let text = |f: &str| {
                self.core
                    .read_field(action_id, f)
                    .and_then(|b| String::from_utf8(b).ok())
            };
            let (Some(status), Some(audit_id)) = (text(action::STATUS), text(action::AUDIT)) else {
                continue;
            };
            if audit_id.is_empty()
                || self.record.signed(&audit_id)
                || self.record.owed.contains_key(&audit_id)
            {
                continue;
            }
            self.record.owed.insert(
                audit_id,
                Owed {
                    action: action_id.to_string(),
                    status,
                },
            );
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
                    let rows = self.normalised(&c, rows);
                    changed |= self.sign_owed(&c, &rows)?;
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

#[cfg(all(test, feature = "demo-unpinned"))]
mod tests {
    use super::*;
    use crate::consoles::common::AuditRecord;
    use crate::demo::enclosure::DemoEnclosure;
    use crate::keystore::SoftwareKeystore;
    use attest::TrustAnchors;

    const BODY: &str = include_str!("../tests/fixtures/session_body.json");

    fn status_of(rows: &[Visible<AuditRecord>], id: &str) -> String {
        rows.iter()
            .find_map(|v| match v {
                Visible::Shown { id: rid, item, .. } if rid == id => {
                    Some(item.pq_signature.clone())
                }
                _ => None,
            })
            .expect("shown")
    }

    #[test]
    fn an_unparseable_device_record_leaves_rows_none() {
        let e =
            DemoEnclosure::from_body("test", &parse(BODY).unwrap(), &TrustAnchors::new()).unwrap();
        let ks = SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap();
        let (device, _) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
        let mut s = Session::open(ks, device, e, "k.rao", "dev-1").unwrap();
        let _ = s.rows::<AuditRecord>("midgard.audit").unwrap();
        let id = s
            .request_action("demo.ping", "midgard.audit/AUD-1", &[])
            .unwrap();
        s.sync().unwrap();
        let ActionState::Applied { audit, .. } = s.action_state(&id) else {
            panic!("not applied")
        };
        let rows = s.rows::<AuditRecord>("midgard.audit").unwrap();
        assert_eq!(status_of(&rows, &audit), "device");

        s.core
            .set_device_audit_record(b"{\"sigs\":", &mut OsRng)
            .unwrap();
        let saved = s.export_state().unwrap();
        let report = s.import_state(&saved.file, saved.generation).unwrap();
        assert!(!report.pending_dropped);
        assert_eq!(s.record, DeviceRecord::default());
        let rows = s.rows::<AuditRecord>("midgard.audit").unwrap();
        assert_eq!(status_of(&rows, &audit), "none");
        s.set_online(false);
        let rows = s.rows::<AuditRecord>("midgard.audit").unwrap();
        assert_eq!(status_of(&rows, &audit), "none");
    }

    #[test]
    fn the_device_record_round_trips() {
        let mut r = DeviceRecord::default();
        r.actions.insert(
            "action:a".into(),
            ActionRecord {
                verb: "demo.ping".into(),
                target: "c/r".into(),
                params_sha256: "00".into(),
            },
        );
        r.owed.insert(
            "AUD-1".into(),
            Owed {
                action: "action:a".into(),
                status: "applied".into(),
            },
        );
        r.sigs.insert(
            "c/AUD-1".into(),
            Signed {
                action: "action:a".into(),
                status: "applied".into(),
                sig: "ab".into(),
            },
        );
        assert_eq!(DeviceRecord::decode(&r.encode()), Some(r));
        assert_eq!(DeviceRecord::decode(b"{}"), None);
    }
}
