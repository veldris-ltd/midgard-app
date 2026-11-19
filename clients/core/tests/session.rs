//! A session reads clearance-filtered rows online and from the sealed cache offline,
//! queues actions offline and syncs them once, refuses a persona switch with unsent
//! work, keeps nothing of one persona after a switch, never shows another persona's
//! rows, discards a state file that does not load, and shows an audit row as
//! `device` only after a check under this device's audit key. [AC-C-13] [FR-C-4]
//! [AC-C-2] [NFR-PQ-3]
#![cfg(feature = "backend-fips")]

use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use attest::json::{parse, Json};
use attest::TrustAnchors;
use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::demo::audit::SignatureStatus;
use midgard_client_core::demo::canon;
use midgard_client_core::demo::enclosure::DemoEnclosure;
use midgard_client_core::device_audit::DeviceAuditKey;
use midgard_client_core::keystore::sealed_root::{RootSealer, SealedRootKeystore};
use midgard_client_core::keystore::{KeystoreError, SoftwareKeystore};
use midgard_client_core::marking::{MarkingLevel, RowMarking, Visible};
use midgard_client_core::session::{ActionState, Session, SessionError, SyncPhase};
use midgard_client_core::source::{
    action, DataSource, Delivered, Grant, Persona, Seal, SourceError, VerifiedDigest,
};
use midgard_client_core::sync::{Replica, SyncOp};
use midgard_client_core::{CoreError, CORE_VERSION};
use pqc_agility::backend::fips::provider;
use pqc_agility::{Aes256GcmAead, OsRng, Rng, SecretBytes, GCM_NONCE_LEN};

const BODY: &str = include_str!("fixtures/session_body.json");

fn enclosure() -> DemoEnclosure {
    DemoEnclosure::from_body("test", &parse(BODY).unwrap(), &TrustAnchors::new()).unwrap()
}

fn session(persona: &str) -> Session<SoftwareKeystore, DemoEnclosure> {
    let ks = SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap();
    let (device, _) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
    Session::open(ks, device, enclosure(), persona, "dev-1").unwrap()
}

fn audit_rows<K, S>(s: &mut Session<K, S>) -> Vec<Visible<AuditRecord>>
where
    K: midgard_client_core::keystore::Keystore + midgard_client_core::keystore::TrustRoot,
    S: DataSource,
{
    s.rows("midgard.audit").unwrap()
}

fn status_of(rows: &[Visible<AuditRecord>], id: &str) -> String {
    rows.iter()
        .find_map(|v| match v {
            Visible::Shown { id: rid, item, .. } if rid == id => Some(item.pq_signature.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{id} not shown"))
}

/// Act on AUD-1, sync, and return the audit row id the source answered with.
fn act_and_sync<K, S>(s: &mut Session<K, S>) -> String
where
    K: midgard_client_core::keystore::Keystore + midgard_client_core::keystore::TrustRoot,
    S: DataSource,
{
    let _ = audit_rows(s);
    let id = s
        .request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    s.sync().unwrap();
    match s.action_state(&id) {
        ActionState::Applied { audit, .. } => audit,
        other => panic!("{other:?}"),
    }
}

#[test]
fn rows_online_then_from_the_sealed_cache_offline() {
    let mut s = session("k.rao");
    let online = audit_rows(&mut s);
    assert!(matches!(
        online[1],
        Visible::Withheld {
            level: MarkingLevel::Confidential
        }
    ));
    // The fixture's "ML-DSA ✓" text is never passed on: nothing checked it.
    assert_eq!(status_of(&online, "AUD-1"), "none");
    s.set_online(false);
    assert_eq!(s.rows::<AuditRecord>("midgard.audit").unwrap(), online);
    assert_eq!(s.sync_status().phase, SyncPhase::Offline);
}

#[test]
fn an_offline_action_queues_then_applies_once() {
    let mut s = session("k.rao");
    let _ = audit_rows(&mut s);
    s.set_online(false);
    let id = s
        .request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    assert!(id.starts_with("action:dev-1-"), "{id}");
    assert_eq!(s.action_state(&id), ActionState::Queued);
    assert!(matches!(s.sync(), Err(SessionError::Offline)));
    assert_eq!(s.sync_status().pending, 4);
    s.set_online(true);
    assert_eq!(s.sync_status().phase, SyncPhase::Syncing);
    let r = s.sync().unwrap();
    assert_eq!((r.pushed, s.sync_status().pending), (4, 0));
    assert!(matches!(s.action_state(&id), ActionState::Applied { .. }));
    assert_eq!(s.sync_status().phase, SyncPhase::Synced);
    assert!(s.sync_status().last_sync.is_some());
    // A second sync pushes nothing and runs nothing again.
    assert_eq!(s.sync().unwrap().pushed, 0);
    let rows = audit_rows(&mut s);
    let made = rows
        .iter()
        .filter(|v| matches!(v, Visible::Shown { item, .. } if item.action.contains("demo.ping")))
        .count();
    assert_eq!(made, 1);
}

#[test]
fn action_ids_never_repeat() {
    let mut s = session("k.rao");
    let _ = audit_rows(&mut s);
    s.set_online(false);
    let a = s
        .request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    let b = s
        .request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    assert_ne!(a, b);
}

#[test]
fn acting_on_a_row_above_clearance_is_refused_before_queueing() {
    let mut s = session("k.rao");
    // Nothing cached yet: the target's marking is unknown, so nothing is queued.
    assert!(matches!(
        s.request_action("demo.ping", "midgard.audit/AUD-1", &[]),
        Err(SessionError::NothingCached(_))
    ));
    let _ = audit_rows(&mut s);
    // AUD-2 is withheld: its marking is the cache's, not the caller's.
    assert!(matches!(
        s.request_action("demo.ping", "midgard.audit/AUD-2", &[]),
        Err(SessionError::AboveClearance)
    ));
    // A row that is not there is refused the same way.
    assert!(matches!(
        s.request_action("demo.ping", "midgard.audit/AUD-9", &[]),
        Err(SessionError::AboveClearance)
    ));
    assert!(matches!(
        s.request_action("demo.ping", "no-slash", &[]),
        Err(SessionError::BadTarget(_))
    ));
    assert_eq!(s.sync_status().pending, 0);
}

#[test]
fn a_switch_with_unsent_work_is_refused_and_a_switch_keeps_nothing() {
    let mut s = session("k.osei");
    let seen = audit_rows(&mut s);
    assert!(matches!(&seen[1], Visible::Shown { id, .. } if id == "AUD-2"));
    s.set_online(false);
    s.request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    let (s, err) = match s.switch_persona("k.rao") {
        Ok(_) => panic!("a switch with pending work must be refused"),
        Err(refused) => refused,
    };
    assert!(matches!(err, SessionError::PendingOps(4)));
    let mut s = s;
    assert_eq!(s.grant().persona().id, "k.osei");
    s.set_online(true);
    s.sync().unwrap();
    let mut s = s.switch_persona("k.rao").map_err(|(_, e)| e).unwrap();
    assert_eq!(s.grant().persona().id, "k.rao");
    s.set_online(false);
    assert!(matches!(
        s.rows::<AuditRecord>("midgard.audit"),
        Err(SessionError::NothingCached(_))
    ));
    // Online, the confidential row is withheld by level only.
    s.set_online(true);
    let rows = audit_rows(&mut s);
    let shown = format!("{rows:?}");
    assert!(
        !shown.contains("AUD-2") && !shown.contains("OPS-N"),
        "{shown}"
    );
    assert!(rows.contains(&Visible::Withheld {
        level: MarkingLevel::Confidential
    }));

    // A switch up keeps nothing either.
    let mut s = s.switch_persona("k.osei").map_err(|(_, e)| e).unwrap();
    s.set_online(false);
    assert!(matches!(
        s.rows::<AuditRecord>("midgard.audit"),
        Err(SessionError::NothingCached(_))
    ));
}

#[test]
fn a_switch_to_an_unknown_persona_keeps_the_session() {
    let s = session("k.rao");
    let (s, err) = match s.switch_persona("nobody") {
        Ok(_) => panic!("unknown persona"),
        Err(r) => r,
    };
    assert!(matches!(
        err,
        SessionError::Source(SourceError::UnknownPersona(_))
    ));
    assert_eq!(s.grant().persona().id, "k.rao");
}

// ---- state file -------------------------------------------------------------

/// A software stand-in for the Secure Enclave key root.
struct TestSealer([u8; 32]);
impl RootSealer for TestSealer {
    fn seal(&self, data_key: &SecretBytes) -> Result<Vec<u8>, KeystoreError> {
        let mut nonce = [0u8; GCM_NONCE_LEN];
        OsRng.fill(&mut nonce);
        let mut out = nonce.to_vec();
        out.extend(
            Aes256GcmAead::new(&self.0)
                .seal(&nonce, b"", data_key.expose())
                .unwrap(),
        );
        Ok(out)
    }
    fn unseal(&self, sealed: &[u8]) -> Result<SecretBytes, KeystoreError> {
        let nonce: [u8; GCM_NONCE_LEN] = sealed[..GCM_NONCE_LEN].try_into().unwrap();
        Aes256GcmAead::new(&self.0)
            .open(&nonce, b"", &sealed[GCM_NONCE_LEN..])
            .map(SecretBytes::new)
            .map_err(|_| KeystoreError::Unseal)
    }
    fn hardware_backed(&self) -> bool {
        false
    }
}

const SEALER: TestSealer = TestSealer([5; 32]);

/// One device: its sealed root and its sealed device-audit key, as the shell
/// stores them.
struct Device {
    root: Vec<u8>,
    audit_key: Vec<u8>,
}

impl Device {
    fn new() -> Self {
        let (ks, root) =
            SealedRootKeystore::create(&provider(), "device-root", &SEALER, &mut OsRng).unwrap();
        let (_, audit_key) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
        Self { root, audit_key }
    }

    fn keystore(&self) -> SealedRootKeystore {
        SealedRootKeystore::open(&self.root, &SEALER).unwrap()
    }

    fn open<S: DataSource>(&self, source: S, persona: &str) -> Session<SealedRootKeystore, S> {
        let ks = self.keystore();
        let device = DeviceAuditKey::open(&ks, &self.audit_key).unwrap();
        Session::open(ks, device, source, persona, "dev-1").unwrap()
    }
}

#[test]
fn state_survives_a_relaunch_with_its_queue_and_generation() {
    let dev = Device::new();
    let mut s = dev.open(enclosure(), "k.rao");
    let rows = audit_rows(&mut s);
    s.set_online(false);
    let id = s
        .request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    let saved = s.export_state().unwrap();
    assert!(saved.generation >= 1);
    let source = s.into_source();

    let mut s = dev.open(source, "k.rao");
    let report = s.import_state(&saved.file, saved.generation).unwrap();
    assert!(!report.pending_dropped);
    assert_eq!(report.generation, saved.generation);
    // AUD-1, the action object and the index; the core's own objects are not counted.
    assert_eq!((report.kept, report.dropped), (3, 0));
    assert_eq!(s.sync_status().pending, 4);
    s.set_online(false);
    assert_eq!(s.rows::<AuditRecord>("midgard.audit").unwrap(), rows);
    s.set_online(true);
    s.sync().unwrap();
    assert!(matches!(s.action_state(&id), ActionState::Applied { .. }));
    // The next file supersedes the one loaded.
    assert!(s.export_state().unwrap().generation > saved.generation);
}

#[test]
fn a_state_file_that_does_not_load_is_discarded_and_the_rows_refetched() {
    let dev = Device::new();
    let mut s = dev.open(enclosure(), "k.rao");
    let rows = audit_rows(&mut s);
    let old = s.export_state().unwrap();
    let new = s.export_state().unwrap();
    let source = s.into_source();

    // A rollback: the older file, with the newer generation the shell kept.
    let mut s = dev.open(source, "k.rao");
    assert!(matches!(
        s.import_state(&old.file, new.generation),
        Err(SessionError::Core(CoreError::Stale { .. }))
    ));
    s.set_online(false);
    assert!(matches!(
        s.rows::<AuditRecord>("midgard.audit"),
        Err(SessionError::NothingCached(_))
    ));
    s.set_online(true);
    assert_eq!(audit_rows(&mut s), rows);

    // A changed byte.
    let source = s.into_source();
    let mut s = dev.open(source, "k.rao");
    let mut bad = new.file.clone();
    let at = bad.len() / 2;
    bad[at] ^= 1;
    assert!(s.import_state(&bad, new.generation).is_err());
    s.set_online(false);
    assert!(matches!(
        s.rows::<AuditRecord>("midgard.audit"),
        Err(SessionError::NothingCached(_))
    ));
}

#[test]
fn another_personas_state_file_does_not_load() {
    let dev = Device::new();
    let mut s = dev.open(enclosure(), "k.osei");
    let _ = audit_rows(&mut s);
    let theirs = s.export_state().unwrap();
    let source = s.into_source();
    let mut s = dev.open(source, "k.rao");
    assert!(s.import_state(&theirs.file, 0).is_err());
    s.set_online(false);
    assert!(matches!(
        s.rows::<AuditRecord>("midgard.audit"),
        Err(SessionError::NothingCached(_))
    ));
}

// ---- device-audit rows ------------------------------------------------------

#[test]
fn a_row_made_for_this_devices_action_is_device_only_after_a_check() {
    let dev = Device::new();
    let mut s = dev.open(enclosure(), "k.rao");
    let audit = act_and_sync(&mut s);
    let rows = audit_rows(&mut s);
    assert_eq!(status_of(&rows, &audit), SignatureStatus::Device.as_str());
    // A built-in row stays as its own check left it.
    assert_eq!(status_of(&rows, "AUD-1"), "none");

    // Offline, from the cache: checked again, still `device`.
    s.set_online(false);
    let cached = s.rows::<AuditRecord>("midgard.audit").unwrap();
    assert_eq!(status_of(&cached, &audit), "device");

    // After a relaunch on the same device: the same key checks it.
    let saved = s.export_state().unwrap();
    let source = s.into_source();
    let mut s = dev.open(source, "k.rao");
    let report = s.import_state(&saved.file, saved.generation).unwrap();
    // AUD-1, the device row, the action object and the index; the device
    // record is not counted.
    assert_eq!(report.kept, 4);
    assert_eq!(status_of(&audit_rows(&mut s), &audit), "device");
}

#[test]
fn a_row_signed_by_another_device_key_is_not_device() {
    let dev = Device::new();
    let mut s = dev.open(enclosure(), "k.rao");
    let audit = act_and_sync(&mut s);
    let saved = s.export_state().unwrap();
    let source = s.into_source();

    // Same root and persona, another device-audit key: the saved signatures are
    // not this key's.
    let ks = dev.keystore();
    let (other, _) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
    let mut s = Session::open(ks, other, source, "k.rao", "dev-1").unwrap();
    s.import_state(&saved.file, saved.generation).unwrap();
    assert_eq!(status_of(&audit_rows(&mut s), &audit), "none");
    s.set_online(false);
    assert_eq!(
        status_of(&s.rows::<AuditRecord>("midgard.audit").unwrap(), &audit),
        "none"
    );
}

#[test]
fn another_personas_device_rows_are_not_device_for_this_one() {
    let mut s = session("k.rao");
    let audit = act_and_sync(&mut s);
    let mut s = s.switch_persona("k.osei").map_err(|(_, e)| e).unwrap();
    assert_eq!(status_of(&audit_rows(&mut s), &audit), "none");
}

/// What a test source does to what it delivers.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Honest,
    /// Changes the actor of every unsigned audit row.
    Tamper,
    /// Claims every unsigned audit row is device-signed.
    ClaimDevice,
    /// Delivers a withheld row in full, above the grant.
    Leak,
    /// Claims every unsigned audit row is verified.
    ClaimVerified,
    /// Fetch fails (exchange still works).
    FailFetch,
    /// Exchange also answers a made-up action "by" this persona, audited as AUD-1.
    Inject,
    /// Exchange also overwrites the target (to AUD-1's resource) and the audit id
    /// (to AUD-1) of each action pushed.
    Overwrite,
    /// Appends a "claimed by" suffix to the `action` text of unsigned audit rows.
    AlterAction,
    /// Reports the audit collection as another type.
    HideAuditType,
    /// Changes the actor of the first verified row.
    AlterVerified,
    /// Exchange also swaps the audit ids of the first two actions pushed.
    SwapAudit,
    /// Exchange also points the second action pushed at the first one's audit id.
    SameAudit,
    /// Exchange drops the audit ids from its answer.
    DropAudit,
}

struct Wrapped {
    inner: DemoEnclosure,
    mode: Cell<Mode>,
    /// Pass the session's seal on to the enclosure inside (as a wrapper can).
    forward: Cell<bool>,
}

impl DataSource for Wrapped {
    fn personas(&self) -> Vec<Persona> {
        self.inner.personas()
    }
    fn collections(&self) -> Vec<(String, String)> {
        let mut c = self.inner.collections();
        if self.mode.get() == Mode::HideAuditType {
            for (_, t) in c.iter_mut() {
                *t = "AdminApproval".into();
            }
        }
        c
    }
    fn grant(&self, persona_id: &str) -> Result<Grant, SourceError> {
        self.inner.grant(persona_id)
    }
    fn fetch(&self, grant: &Grant, collection: &str) -> Result<Vec<Delivered>, SourceError> {
        let mode = self.mode.get();
        if mode == Mode::FailFetch {
            return Err(SourceError::Offline);
        }
        let rows = self.inner.fetch(grant, collection)?;
        let mut first_verified = true;
        Ok(rows
            .into_iter()
            .map(|d| match (mode, d) {
                (
                    Mode::Tamper,
                    Delivered::Row {
                        id,
                        marking,
                        body,
                        signature: Some(SignatureStatus::Unsigned),
                    },
                ) => {
                    let v = parse(std::str::from_utf8(&body).unwrap()).unwrap();
                    let Json::Obj(pairs) = v else { unreachable!() };
                    let pairs = pairs
                        .into_iter()
                        .map(|(k, v)| {
                            if k == "actor" {
                                (k, Json::Str("m.mallory".into()))
                            } else {
                                (k, v)
                            }
                        })
                        .collect();
                    Delivered::Row {
                        id,
                        marking,
                        body: canon::encode(&Json::Obj(pairs)).into_bytes(),
                        signature: Some(SignatureStatus::Unsigned),
                    }
                }
                (
                    Mode::ClaimDevice,
                    Delivered::Row {
                        id,
                        marking,
                        body,
                        signature: Some(SignatureStatus::Unsigned),
                    },
                ) => Delivered::Row {
                    id,
                    marking,
                    body: String::from_utf8(body)
                        .unwrap()
                        .replace("\"none\"", "\"device\"")
                        .into_bytes(),
                    signature: Some(SignatureStatus::Device),
                },
                (Mode::Leak, Delivered::Withheld { level }) => Delivered::Row {
                    id: "AUD-2".into(),
                    marking: RowMarking::new(level, ["OPS-N"]),
                    body: br#"{"id":"AUD-2","actor":"x","action":"y","resource":"z","ts":"14:00:01","pqSignature":"none"}"#.to_vec(),
                    signature: Some(SignatureStatus::Unsigned),
                },
                (
                    Mode::ClaimVerified,
                    Delivered::Row {
                        id,
                        marking,
                        body,
                        signature: Some(SignatureStatus::Unsigned),
                    },
                ) => Delivered::Row {
                    id,
                    marking,
                    body: String::from_utf8(body)
                        .unwrap()
                        .replace("\"none\"", "\"verified\"")
                        .into_bytes(),
                    signature: Some(SignatureStatus::Verified),
                },
                (
                    Mode::AlterAction,
                    Delivered::Row {
                        id,
                        marking,
                        body,
                        signature: Some(SignatureStatus::Unsigned),
                    },
                ) => Delivered::Row {
                    id,
                    marking,
                    body: String::from_utf8(body)
                        .unwrap()
                        .replace("demo.ping\"", "demo.ping (claimed by k.rao)\"")
                        .into_bytes(),
                    signature: Some(SignatureStatus::Unsigned),
                },
                (
                    Mode::AlterVerified,
                    Delivered::Row {
                        id,
                        marking,
                        body,
                        signature: Some(SignatureStatus::Verified),
                    },
                ) if first_verified => {
                    first_verified = false;
                    let v = parse(std::str::from_utf8(&body).unwrap()).unwrap();
                    let Json::Obj(pairs) = v else { unreachable!() };
                    let pairs = pairs
                        .into_iter()
                        .map(|(k, v)| {
                            if k == "actor" {
                                (k, Json::Str("m.mallory".into()))
                            } else {
                                (k, v)
                            }
                        })
                        .collect();
                    Delivered::Row {
                        id,
                        marking,
                        body: canon::encode(&Json::Obj(pairs)).into_bytes(),
                        signature: Some(SignatureStatus::Verified),
                    }
                }
                (_, d) => d,
            })
            .collect())
    }
    fn exchange(&mut self, grant: &Grant, ops: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, SourceError> {
        let mut back = self.inner.exchange(grant, ops)?;
        // The audit id the enclosure answered for each action, in action order.
        let audits: Vec<(String, Vec<u8>)> = back
            .iter()
            .map(|b| SyncOp::decode(b).unwrap())
            .filter(|o| o.field == action::AUDIT)
            .map(|o| (o.object, o.value.unwrap_or_default()))
            .collect();
        if self.mode.get() == Mode::DropAudit {
            back.retain(|b| SyncOp::decode(b).unwrap().field != action::AUDIT);
        }
        let mut evil = Replica::new("evil");
        evil.observe(1 << 40);
        match self.mode.get() {
            Mode::Inject => {
                let made_up = "action:dev-1-0000000000000000-999";
                for (f, v) in [
                    (action::VERB, "demo.ping"),
                    (action::TARGET, "r"),
                    (action::PARAMS, "{}"),
                    (action::BY, "k.rao"),
                    (action::STATUS, "applied"),
                    (action::AUDIT, "AUD-1"),
                ] {
                    evil.set(made_up, f, v.as_bytes().to_vec());
                }
            }
            Mode::SwapAudit if audits.len() >= 2 => {
                evil.set(&audits[0].0, action::AUDIT, audits[1].1.clone());
                evil.set(&audits[1].0, action::AUDIT, audits[0].1.clone());
            }
            Mode::SameAudit if audits.len() >= 2 => {
                evil.set(&audits[1].0, action::AUDIT, audits[0].1.clone());
            }
            Mode::Overwrite => {
                let ids: BTreeSet<String> = ops
                    .iter()
                    .map(|b| SyncOp::decode(b).unwrap().object)
                    .collect();
                for id in ids {
                    evil.set(&id, action::TARGET, b"r".to_vec());
                    evil.set(&id, action::AUDIT, b"AUD-1".to_vec());
                }
            }
            _ => {}
        }
        back.extend(evil.drain_pending().iter().map(SyncOp::encode));
        Ok(back)
    }
    fn online(&self) -> bool {
        self.inner.online()
    }
    fn set_online(&mut self, online: bool) {
        self.inner.set_online(online)
    }
    fn in_core_verified(
        &self,
        seal: &Seal,
        grant: &Grant,
        collection: &str,
        id: &str,
    ) -> Option<VerifiedDigest> {
        if self.forward.get() {
            self.inner.in_core_verified(seal, grant, collection, id)
        } else {
            None
        }
    }
}

fn wrapped() -> Wrapped {
    wrap(enclosure())
}

fn wrap(inner: DemoEnclosure) -> Wrapped {
    Wrapped {
        inner,
        mode: Cell::new(Mode::Honest),
        forward: Cell::new(false),
    }
}

fn pinned() -> DemoEnclosure {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../demo/datasets/{CORE_VERSION}.mgdemo"));
    DemoEnclosure::open_pinned(&fs::read(path).unwrap()).unwrap()
}

fn statuses(rows: &[Visible<AuditRecord>]) -> BTreeSet<String> {
    rows.iter()
        .filter_map(|v| v.shown().map(|r| r.pq_signature.clone()))
        .collect()
}

/// The newest audit row (the source puts new rows first).
fn newest(rows: &[Visible<AuditRecord>]) -> (String, String) {
    match &rows[0] {
        Visible::Shown { id, item, .. } => (id.clone(), item.pq_signature.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_tampered_device_row_is_not_device() {
    let dev = Device::new();
    let mut s = dev.open(wrapped(), "k.rao");
    let audit = act_and_sync(&mut s);
    assert_eq!(status_of(&audit_rows(&mut s), &audit), "device");
    s.source().mode.set(Mode::Tamper);
    let rows = audit_rows(&mut s);
    assert_eq!(status_of(&rows, &audit), "none");
    // Cached as delivered, so offline it is not `device` either.
    s.set_online(false);
    assert_eq!(
        status_of(&s.rows::<AuditRecord>("midgard.audit").unwrap(), &audit),
        "none"
    );
}

#[test]
fn a_source_cannot_claim_a_row_is_device_signed() {
    let dev = Device::new();
    let mut s = dev.open(wrapped(), "k.rao");
    s.source().mode.set(Mode::ClaimDevice);
    // Nothing of this device's: a claimed `device` row is shown as `none`.
    let rows = audit_rows(&mut s);
    assert_eq!(status_of(&rows, "AUD-1"), "none");
    s.set_online(false);
    assert_eq!(
        status_of(&s.rows::<AuditRecord>("midgard.audit").unwrap(), "AUD-1"),
        "none"
    );
}

#[test]
fn a_source_that_leaks_a_row_above_clearance_is_refused() {
    let dev = Device::new();
    let mut s = dev.open(wrapped(), "k.rao");
    s.source().mode.set(Mode::Leak);
    assert!(matches!(
        s.rows::<AuditRecord>("midgard.audit"),
        Err(SessionError::AboveClearance)
    ));
    s.set_online(false);
    assert!(matches!(
        s.rows::<AuditRecord>("midgard.audit"),
        Err(SessionError::NothingCached(_))
    ));
}

#[test]
fn a_made_up_action_in_the_answer_gets_nothing_signed() {
    let dev = Device::new();
    let mut s = dev.open(wrapped(), "k.rao");
    s.source().mode.set(Mode::Inject);
    let audit = act_and_sync(&mut s);
    s.source().mode.set(Mode::Honest);
    let rows = audit_rows(&mut s);
    // AUD-1's actor is this persona and its resource the made-up action's target,
    // but the action is not one this device recorded.
    assert_eq!(status_of(&rows, "AUD-1"), "none");
    // The genuine action's row is still this device's.
    assert_eq!(status_of(&rows, &audit), "device");
}

#[test]
fn an_overwritten_target_and_audit_id_get_nothing_signed() {
    let dev = Device::new();
    let mut s = dev.open(wrapped(), "k.rao");
    let _ = audit_rows(&mut s);
    s.source().mode.set(Mode::Overwrite);
    let id = s
        .request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    s.sync().unwrap();
    // The merged view now names AUD-1, whose resource is the overwritten target.
    assert!(matches!(s.action_state(&id), ActionState::Applied { audit, .. } if audit == "AUD-1"));
    s.source().mode.set(Mode::Honest);
    let rows = audit_rows(&mut s);
    assert_eq!(status_of(&rows, "AUD-1"), "none");
    assert_eq!(newest(&rows).1, "none");
}

#[test]
fn a_row_whose_action_text_is_not_the_answer_is_not_signed() {
    let dev = Device::new();
    let mut s = dev.open(wrapped(), "k.rao");
    s.source().mode.set(Mode::AlterAction);
    let audit = act_and_sync(&mut s);
    assert_eq!(status_of(&audit_rows(&mut s), &audit), "none");
    s.source().mode.set(Mode::Honest);
    assert_eq!(status_of(&audit_rows(&mut s), &audit), "none");
}

#[test]
fn an_owed_row_is_signed_on_its_first_later_read() {
    let dev = Device::new();
    let mut s = dev.open(wrapped(), "k.rao");
    let _ = audit_rows(&mut s);
    let id = s
        .request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    s.source().mode.set(Mode::FailFetch);
    s.sync().unwrap();
    let ActionState::Applied { audit, .. } = s.action_state(&id) else {
        panic!("not applied")
    };
    // Kept across a relaunch, too.
    let saved = s.export_state().unwrap();
    let source = s.into_source();
    let mut s = dev.open(source, "k.rao");
    s.import_state(&saved.file, saved.generation).unwrap();
    s.source().mode.set(Mode::Honest);
    assert_eq!(status_of(&audit_rows(&mut s), &audit), "device");
}

#[test]
fn rows_of_a_collection_that_is_not_an_audit_type_are_never_signed() {
    let dev = Device::new();
    let mut s = dev.open(wrapped(), "k.rao");
    s.source().mode.set(Mode::HideAuditType);
    let audit = act_and_sync(&mut s);
    // Owed, but the collection is not an audit collection: not signed, not checked.
    assert_eq!(status_of(&audit_rows(&mut s), &audit), "none");
    s.set_online(false);
    assert_eq!(
        status_of(&s.rows::<AuditRecord>("midgard.audit").unwrap(), &audit),
        "none"
    );
}

#[test]
fn only_an_in_core_source_may_say_verified() {
    // The demo enclosure checked the built-in rows under the pinned anchor.
    let ks = SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap();
    let (device, _) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
    let mut s = Session::open(ks, device, pinned(), "k.osei", "dev-1").unwrap();
    assert!(statuses(&audit_rows(&mut s)).contains("verified"));

    let direct = audit_rows(&mut s);

    // The same rows through a source outside the core: not believed.
    let dev = Device::new();
    let mut s = dev.open(wrap(pinned()), "k.osei");
    let st = statuses(&audit_rows(&mut s));
    assert!(!st.contains("verified"), "{st:?}");

    // A wrapper that passes the seal on: each row is believed only as the
    // enclosure verified it, so unaltered rows stay verified and an altered one
    // does not.
    let mut s = dev.open(wrap(pinned()), "k.osei");
    s.source().forward.set(true);
    assert_eq!(audit_rows(&mut s), direct);
    s.source().mode.set(Mode::AlterVerified);
    let altered = audit_rows(&mut s);
    let mut changed = 0;
    for (a, d) in altered.iter().zip(&direct) {
        match (a, d) {
            (Visible::Shown { item: a, .. }, Visible::Shown { item: d, .. })
                if a.actor != d.actor =>
            {
                assert_eq!(d.pq_signature, "verified");
                assert_eq!(a.pq_signature, "none");
                changed += 1;
            }
            _ => assert_eq!(a, d),
        }
    }
    assert_eq!(changed, 1);
    // Offline, the cache keeps what was decided online.
    s.set_online(false);
    assert_eq!(s.rows::<AuditRecord>("midgard.audit").unwrap(), altered);

    // Nor a claim on rows nobody signed.
    let mut s = dev.open(wrapped(), "k.rao");
    s.source().mode.set(Mode::ClaimVerified);
    assert_eq!(status_of(&audit_rows(&mut s), "AUD-1"), "none");
}

#[test]
fn unsent_actions_lost_on_import_are_reported() {
    let dev = Device::new();
    let mut s = dev.open(enclosure(), "k.osei");
    let _ = audit_rows(&mut s);
    s.set_online(false);
    s.request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    let saved = s.export_state().unwrap();

    // The same persona, now at a lower clearance: the queue (sealed at the old
    // clearance) cannot be read, and the caller is told.
    let lower = BODY.replace(
        r#""clearance":"confidential","compartments":["OPS-N","SIG"]"#,
        r#""clearance":"restricted","compartments":[]"#,
    );
    assert_ne!(lower, BODY);
    let e =
        DemoEnclosure::from_body("test", &parse(&lower).unwrap(), &TrustAnchors::new()).unwrap();
    let mut s = dev.open(e, "k.osei");
    let report = s.import_state(&saved.file, saved.generation).unwrap();
    assert!(report.pending_dropped);
    assert_eq!(s.sync_status().pending, 0);
}

/// Two `demo.ping` actions on AUD-1, synced together; the two newest audit rows.
fn two_actions(mode: Mode) -> (Vec<Visible<AuditRecord>>, (usize, usize, usize)) {
    let dev = Device::new();
    let mut s = dev.open(wrapped(), "k.rao");
    let _ = audit_rows(&mut s);
    s.request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    s.request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    s.source().mode.set(mode);
    s.sync().unwrap();
    s.source().mode.set(Mode::Honest);
    let rows = audit_rows(&mut s);
    (rows[..2].to_vec(), s.device_record_sizes_for_tests())
}

#[test]
fn two_actions_bound_honestly_are_both_device() {
    let (rows, sizes) = two_actions(Mode::Honest);
    assert_eq!(statuses(&rows), BTreeSet::from(["device".to_string()]));
    // Decided actions leave the record; the two signatures stay.
    assert_eq!(sizes, (0, 0, 2));
}

#[test]
fn swapped_audit_ids_get_neither_row_signed() {
    let (rows, sizes) = two_actions(Mode::SwapAudit);
    assert_eq!(statuses(&rows), BTreeSet::from(["none".to_string()]));
    assert_eq!(sizes, (0, 0, 0));
}

#[test]
fn one_audit_id_claimed_by_two_actions_gets_neither_signed() {
    let (rows, sizes) = two_actions(Mode::SameAudit);
    assert_eq!(statuses(&rows), BTreeSet::from(["none".to_string()]));
    assert_eq!(sizes, (0, 0, 0));
}

#[test]
fn decided_actions_leave_the_record() {
    // Answered without an audit row.
    let dev = Device::new();
    let mut s = dev.open(wrapped(), "k.rao");
    let _ = audit_rows(&mut s);
    s.request_action("demo.ping", "midgard.audit/AUD-1", &[])
        .unwrap();
    assert_eq!(s.device_record_sizes_for_tests(), (1, 0, 0));
    s.source().mode.set(Mode::DropAudit);
    s.sync().unwrap();
    assert_eq!(s.device_record_sizes_for_tests(), (0, 0, 0));

    // Consumed without a match.
    let mut s = dev.open(wrapped(), "k.rao");
    s.source().mode.set(Mode::AlterAction);
    let _ = act_and_sync(&mut s);
    assert_eq!(s.device_record_sizes_for_tests(), (0, 0, 0));

    // Signed.
    let mut s = dev.open(wrapped(), "k.rao");
    let _ = act_and_sync(&mut s);
    assert_eq!(s.device_record_sizes_for_tests(), (0, 0, 1));
}
