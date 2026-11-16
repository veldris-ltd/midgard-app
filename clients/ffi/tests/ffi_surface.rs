//! The binding surface, exercised from Rust exactly as Swift calls it: enrol, unlock,
//! typed rows with withheld entries, offline queue, sync, device-signed audit rows,
//! state export and import, lock, and every error a screen tells apart.
//! [FR-C-1] [DD-6] [AC-C-13] [FR-C-4] [NFR-PQ-3]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use midgard_client_ffi::*;
use pqc_agility::{Aes256GcmAead, OsRng, Rng, GCM_NONCE_LEN};

/// How the test key root answers `unseal`.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Honest,
    /// Seals the data key in the clear (a broken platform side).
    Plaintext,
    /// Unseals to a different 32-byte key (another platform key).
    WrongKey,
    /// `unseal` reports the sealed bytes malformed.
    UnsealMalformed,
    /// `unseal` reports the platform key unavailable.
    UnsealUnavailable,
}

/// A software platform key: AES-256-GCM under a fixed key, like the Enclave's ECIES
/// in shape (nonce | ciphertext | tag, 60 bytes). Counts its unseals (prompts).
struct TestKeyRoot {
    key: [u8; 32],
    mode: Mode,
    cancel: AtomicBool,
    /// `unseal` panics (a platform side that crashes mid-prompt).
    panic: AtomicBool,
    prompts: AtomicUsize,
    /// When set, `unseal` calls back into this client (as a careless shell might)
    /// and records what it saw.
    reenter: OnceLock<Weak<MidgardClient>>,
    saw_locked: AtomicBool,
}

impl TestKeyRoot {
    fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            key: [7; 32],
            mode,
            cancel: AtomicBool::new(false),
            panic: AtomicBool::new(false),
            prompts: AtomicUsize::new(0),
            reenter: OnceLock::new(),
            saw_locked: AtomicBool::new(false),
        })
    }
}

impl KeyRoot for TestKeyRoot {
    fn seal(&self, data_key: Vec<u8>) -> Result<Vec<u8>, KeyRootError> {
        if self.mode == Mode::Plaintext {
            return Ok([data_key.clone(), vec![0; 16]].concat());
        }
        let mut nonce = [0u8; GCM_NONCE_LEN];
        OsRng.fill(&mut nonce);
        let ct = Aes256GcmAead::new(&self.key)
            .seal(&nonce, b"test-key-root", &data_key)
            .map_err(|e| KeyRootError::PlatformUnavailable {
                detail: e.to_string(),
            })?;
        Ok([nonce.to_vec(), ct].concat())
    }

    fn unseal(&self, sealed: Vec<u8>) -> Result<Vec<u8>, KeyRootError> {
        self.prompts.fetch_add(1, Ordering::SeqCst);
        if let Some(c) = self.reenter.get().and_then(Weak::upgrade) {
            let locked = c.is_locked()
                && c.personas().len() == 4
                && matches!(c.status_chips(), Err(ClientError::Locked));
            self.saw_locked.store(locked, Ordering::SeqCst);
        }
        if self.panic.load(Ordering::SeqCst) {
            panic!("platform side crashed");
        }
        if self.cancel.load(Ordering::SeqCst) {
            return Err(KeyRootError::UserCancelled);
        }
        if sealed.len() < GCM_NONCE_LEN + 16 {
            return Err(KeyRootError::Malformed {
                detail: "short".into(),
            });
        }
        match self.mode {
            Mode::WrongKey => return Ok(vec![9; 32]),
            Mode::UnsealMalformed => {
                return Err(KeyRootError::Malformed {
                    detail: "not ours".into(),
                })
            }
            Mode::UnsealUnavailable => {
                return Err(KeyRootError::PlatformUnavailable {
                    detail: "key deleted".into(),
                })
            }
            _ => {}
        }
        let (nonce, ct) = sealed.split_at(GCM_NONCE_LEN);
        Aes256GcmAead::new(&self.key)
            .open(nonce.try_into().unwrap(), b"test-key-root", ct)
            .map_err(|_| KeyRootError::Unseal)
    }

    fn hardware_backed(&self) -> bool {
        false
    }
}

fn dataset() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../clients/demo/datasets")
        .join(format!("{}.mgdemo", core_version()))
}

fn config(path: &std::path::Path) -> ClientConfig {
    ClientConfig {
        dataset_path: path.display().to_string(),
        replica_id: "test-device".into(),
        device: DeviceInfo {
            ram_mb: 8192,
            has_accelerator: true,
        },
    }
}

fn client(root: Arc<TestKeyRoot>) -> Arc<MidgardClient> {
    MidgardClient::new(config(&dataset()), root).unwrap()
}

/// A client, enrolled and signed in as `persona`.
fn signed_in(persona: &str) -> (Arc<MidgardClient>, Arc<TestKeyRoot>, SealedKeys) {
    let root = TestKeyRoot::new(Mode::Honest);
    let c = client(root.clone());
    let keys = c.enrol().unwrap();
    c.unlock(keys.clone(), persona.into()).unwrap();
    (c, root, keys)
}

fn first_shown(rows: &[ConsoleRow]) -> String {
    rows.iter()
        .find(|r| !r.withheld)
        .unwrap()
        .id
        .clone()
        .unwrap()
}

#[test]
fn the_whole_session_surface() {
    let root = TestKeyRoot::new(Mode::Honest);
    let c = client(root.clone());
    assert_eq!(c.personas().len(), 4);
    assert!(c.is_locked());
    assert!(c
        .collections()
        .iter()
        .any(|k| k.name == "heimdall.motion" && k.row_type == "HeimdallMotion"));
    let keys = c.enrol().unwrap();
    assert_eq!(
        root.prompts.load(Ordering::SeqCst),
        0,
        "enrolment never prompts"
    );
    let info = c.unlock(keys.clone(), "k.rao".into()).unwrap();
    assert_eq!(
        root.prompts.load(Ordering::SeqCst),
        1,
        "one prompt per unlock"
    );
    assert_eq!(info.persona.id, "k.rao");
    assert_eq!(info.persona.clearance_label, "RESTRICTED");
    assert_eq!(info.chips.custody.floor, "NonExtractable");
    assert!(!info.chips.custody.hardware_wrapped);
    assert_eq!(info.chips.tier.tier, InferenceTier::Remote);

    // Typed rows; above-clearance rows by level only.
    let rows = c.collection_rows("midgard.objectTypes".into()).unwrap();
    let withheld: Vec<_> = rows.iter().filter(|r| r.withheld).collect();
    assert_eq!(
        withheld.len(),
        2,
        "k.rao (restricted) has two confidential rows withheld"
    );
    for w in withheld {
        assert!(w.id.is_none() && w.item.is_none() && w.signature.is_none());
        assert_eq!(w.marking.level, MarkingLevelChip::Confidential);
        assert_eq!(w.marking.label, "CONFIDENTIAL");
    }
    let shown = rows.iter().find(|r| !r.withheld).unwrap();
    assert!(
        matches!(&shown.item, Some(RowItem::MidgardObjectType { item }) if !item.name.is_empty())
    );
    assert!(
        shown.signature.is_none(),
        "only audit rows carry a signature status"
    );
    let visible = first_shown(&rows);

    // The motion hour (hand-written read model) crosses too.
    let motion = c.collection_rows("heimdall.motion".into()).unwrap();
    let Some(RowItem::HeimdallMotion { item }) = motion.iter().find_map(|r| r.item.clone()) else {
        panic!("no motion row shown")
    };
    assert!(!item.samples.is_empty());
    assert!(item.samples.windows(2).all(|w| w[0].t < w[1].t));

    // Offline: AI off, actions queue, sync refused.
    let chips = c.set_offline(true).unwrap();
    assert_eq!(chips.sync.phase, SyncPhase::Offline);
    assert_eq!(chips.tier.tier, InferenceTier::Disabled);
    assert_eq!(chips.tier.reason, "AI off");
    let id = c
        .request_action(ActionInput {
            verb: "demo.ping".into(),
            target: format!("midgard.objectTypes/{visible}"),
            params: vec![KeyValue {
                key: "note".into(),
                value: "hello".into(),
            }],
        })
        .unwrap();
    assert_eq!(
        c.action_status(id.clone()).unwrap().state,
        ActionState::Queued
    );
    assert!(matches!(c.sync_now(), Err(ClientError::Offline)));
    assert_eq!(c.status_chips().unwrap().sync.pending, 4);
    c.set_offline(false).unwrap();
    let report = c.sync_now().unwrap();
    assert_eq!(report.pushed, 4);
    assert_eq!(report.chips.sync.phase, SyncPhase::Synced);
    let status = c.action_status(id).unwrap();
    assert_eq!(status.state, ActionState::Applied);
    let audit_id = status.audit_id.unwrap();

    // The audit row for this device's action is device-signed; the built-in rows are
    // verified under the pinned demo-audit anchor.
    let audit = c.collection_rows("midgard.audit".into()).unwrap();
    let sig = |id: &str| {
        audit
            .iter()
            .find(|r| r.id.as_deref() == Some(id))
            .and_then(|r| r.signature)
    };
    assert_eq!(sig(&audit_id), Some(SignatureStatus::Device));
    let builtin = audit
        .iter()
        .find(|r| r.id.as_deref() != Some(audit_id.as_str()))
        .unwrap();
    assert_eq!(builtin.signature, Some(SignatureStatus::Verified));
    let Some(RowItem::AuditRecord { item }) = &builtin.item else {
        panic!("not an audit row")
    };
    assert_eq!(item.pq_signature, "verified");

    // Lock drops the session; unlocking prompts again.
    c.lock().unwrap();
    assert!(c.is_locked());
    assert!(matches!(
        c.collection_rows("midgard.objectTypes".into()),
        Err(ClientError::Locked)
    ));
    c.unlock(keys, "k.rao".into()).unwrap();
    assert_eq!(root.prompts.load(Ordering::SeqCst), 2);
    assert!(matches!(
        c.unlock(c.enrol().unwrap(), "k.rao".into()),
        Err(ClientError::AlreadyUnlocked)
    ));
}

#[test]
fn state_round_trips_and_a_rollback_is_refused() {
    let (c, _root, keys) = signed_in("k.rao");
    let rows = c.collection_rows("midgard.objectTypes".into()).unwrap();
    let visible = first_shown(&rows);
    c.set_offline(true).unwrap();
    c.request_action(ActionInput {
        verb: "demo.ping".into(),
        target: format!("midgard.objectTypes/{visible}"),
        params: vec![],
    })
    .unwrap();
    let first = c.export_state().unwrap();
    let second = c.export_state().unwrap();
    assert!(second.generation > first.generation);

    // Relaunch: a fresh client, the same keys, the newest file.
    c.lock().unwrap();
    let c = client(TestKeyRoot::new(Mode::Honest));
    c.unlock(keys, "k.rao".into()).unwrap();
    c.set_offline(true).unwrap();
    let report = c
        .import_state(second.file.clone(), second.generation)
        .unwrap();
    assert!(!report.pending_dropped);
    assert_eq!(report.generation, second.generation);
    assert!(report.kept > 0);
    assert_eq!(
        c.status_chips().unwrap().sync.pending,
        4,
        "the queue survives"
    );
    assert_eq!(
        c.collection_rows("midgard.objectTypes".into())
            .unwrap()
            .len(),
        rows.len()
    );

    // An older file than the newest the shell kept is a rollback.
    match c.import_state(first.file, second.generation) {
        Err(ClientError::Stale { found, min }) => {
            assert_eq!(found, first.generation);
            assert_eq!(min, second.generation);
        }
        other => panic!("expected Stale, got {other:?}"),
    }
    assert!(matches!(
        c.import_state(b"not a state file".to_vec(), 0),
        Err(ClientError::Integrity { .. } | ClientError::Malformed { .. })
    ));
}

#[test]
fn every_action_refusal_is_its_own_error() {
    let (c, _root, _keys) = signed_in("k.rao");
    let act = |target: &str| {
        c.request_action(ActionInput {
            verb: "demo.ping".into(),
            target: target.into(),
            params: vec![],
        })
    };
    assert!(
        matches!(act("no-slash"), Err(ClientError::BadTarget { target }) if target == "no-slash")
    );
    assert!(matches!(
        act("midgard.objectTypes/anything"),
        Err(ClientError::NothingCached { collection }) if collection == "midgard.objectTypes"
    ));
    c.collection_rows("midgard.objectTypes".into()).unwrap();
    assert!(matches!(
        act("midgard.objectTypes/no-such-row"),
        Err(ClientError::AboveClearance)
    ));
    assert!(matches!(
        c.collection_rows("no.such".into()),
        Err(ClientError::UnknownCollection { collection }) if collection == "no.such"
    ));

    // A switch with unsent work is refused, and the session stays as it was.
    let rows = c.collection_rows("midgard.objectTypes".into()).unwrap();
    c.set_offline(true).unwrap();
    act(&format!("midgard.objectTypes/{}", first_shown(&rows))).unwrap();
    assert!(matches!(
        c.switch_persona("j.benjamin".into()),
        Err(ClientError::PendingOps { count: 4 })
    ));
    assert!(!c.is_locked());
    c.set_offline(false).unwrap();
    c.sync_now().unwrap();
    let info = c.switch_persona("j.benjamin".into()).unwrap();
    assert_eq!(info.persona.id, "j.benjamin");
    assert_eq!(info.persona.clearance_label, "CONFIDENTIAL // OPS-N");
    assert!(matches!(
        c.switch_persona("nobody".into()),
        Err(ClientError::UnknownPersona { persona }) if persona == "nobody"
    ));
    assert_eq!(c.session_persona().unwrap().id, "j.benjamin");
}

#[test]
fn an_unknown_persona_never_prompts_and_the_client_stays_usable() {
    let root = TestKeyRoot::new(Mode::Honest);
    let c = client(root.clone());
    let keys = c.enrol().unwrap();
    assert!(matches!(
        c.unlock(keys.clone(), "nobody".into()),
        Err(ClientError::UnknownPersona { .. })
    ));
    assert_eq!(root.prompts.load(Ordering::SeqCst), 0);
    assert!(c.is_locked());
    c.unlock(keys, "k.rao".into()).unwrap();
}

#[test]
fn a_cancelled_prompt_leaves_the_client_locked() {
    let root = TestKeyRoot::new(Mode::Honest);
    let c = client(root.clone());
    let keys = c.enrol().unwrap();
    root.cancel.store(true, Ordering::SeqCst);
    assert!(matches!(
        c.unlock(keys.clone(), "k.rao".into()),
        Err(ClientError::Cancelled)
    ));
    assert!(c.is_locked());
    root.cancel.store(false, Ordering::SeqCst);
    c.unlock(keys, "k.rao".into()).unwrap();
}

#[test]
fn key_root_failures_are_told_apart() {
    // Another platform key: the root does not open.
    let honest = client(TestKeyRoot::new(Mode::Honest));
    let keys = honest.enrol().unwrap();
    let other = client(TestKeyRoot::new(Mode::WrongKey));
    assert!(matches!(
        other.unlock(keys.clone(), "k.rao".into()),
        Err(ClientError::UnsealFailed)
    ));
    assert!(other.is_locked());

    // A corrupt sealed root is refused before the platform side is asked.
    let root = TestKeyRoot::new(Mode::Honest);
    let c = client(root.clone());
    let mut bad = keys.clone();
    bad.root.truncate(20);
    assert!(matches!(
        c.unlock(bad, "k.rao".into()),
        Err(ClientError::SealedRootMalformed { .. })
    ));
    assert_eq!(root.prompts.load(Ordering::SeqCst), 0);

    // A changed device-audit key blob.
    let mut bad = keys.clone();
    let mid = bad.device_audit.len() / 2;
    bad.device_audit[mid] ^= 1;
    assert!(matches!(
        c.unlock(bad, "k.rao".into()),
        Err(ClientError::DeviceKey { .. })
    ));
    assert!(c.is_locked());

    // A platform side that hands the key back unsealed.
    let leaky = client(TestKeyRoot::new(Mode::Plaintext));
    assert!(matches!(
        leaky.enrol(),
        Err(ClientError::KeyRootUnavailable { .. })
    ));
}

#[test]
fn a_tampered_dataset_is_refused_at_open() {
    let mut bytes = std::fs::read(dataset()).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 1;
    let dir = std::env::temp_dir().join(format!("midgard-ffi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("tampered.mgdemo");
    std::fs::write(&bad, bytes).unwrap();
    let r = MidgardClient::new(config(&bad), TestKeyRoot::new(Mode::Honest));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(matches!(r, Err(ClientError::SignatureFailure { .. })));
    assert!(matches!(
        MidgardClient::new(config(&dir.join("missing")), TestKeyRoot::new(Mode::Honest)),
        Err(ClientError::NotFound { .. })
    ));
}

/// The error names the file, never the directory it was looked for in (the app's
/// container path, the user name). [FR-C-9]
#[test]
fn a_missing_dataset_is_named_by_file_name_only() {
    let dir = std::env::temp_dir().join(format!("midgard-ffi-path-{}", std::process::id()));
    match MidgardClient::new(
        config(&dir.join("absent.mgdemo")),
        TestKeyRoot::new(Mode::Honest),
    ) {
        Err(ClientError::NotFound { what }) => {
            assert!(what.contains("absent.mgdemo"), "{what}");
            assert!(!what.contains(&*dir.to_string_lossy()), "{what}");
            assert!(!what.contains('/'), "{what}");
        }
        Err(other) => panic!("expected NotFound, got {other:?}"),
        Ok(_) => panic!("expected NotFound, got a client"),
    }
}

#[test]
fn reset_demo_locks_and_reloads_the_dataset() {
    let (c, _root, keys) = signed_in("k.rao");
    let rows = c.collection_rows("midgard.audit".into()).unwrap();
    let rows_before = rows.len();
    let visible = first_shown(&c.collection_rows("midgard.objectTypes".into()).unwrap());
    c.request_action(ActionInput {
        verb: "demo.ping".into(),
        target: format!("midgard.objectTypes/{visible}"),
        params: vec![],
    })
    .unwrap();
    c.sync_now().unwrap();
    assert_eq!(
        c.collection_rows("midgard.audit".into()).unwrap().len(),
        rows_before + 1
    );
    c.reset_demo().unwrap();
    assert!(c.is_locked());
    c.unlock(keys, "k.rao".into()).unwrap();
    assert_eq!(
        c.collection_rows("midgard.audit".into()).unwrap().len(),
        rows_before
    );
}

#[test]
fn the_key_root_may_read_the_client_during_the_prompt_and_sees_it_locked() {
    let root = TestKeyRoot::new(Mode::Honest);
    let c = client(root.clone());
    let keys = c.enrol().unwrap();
    root.reenter.set(Arc::downgrade(&c)).unwrap();
    // Would deadlock if unlock held the client's lock across the prompt.
    c.unlock(keys.clone(), "k.rao".into()).unwrap();
    assert!(root.saw_locked.load(Ordering::SeqCst));
    assert!(!c.is_locked());
}

#[test]
fn unseal_failures_from_the_platform_map_through_unlock() {
    let keys = client(TestKeyRoot::new(Mode::Honest)).enrol().unwrap();
    let c = client(TestKeyRoot::new(Mode::UnsealMalformed));
    assert!(matches!(
        c.unlock(keys.clone(), "k.rao".into()),
        Err(ClientError::SealedRootMalformed { detail }) if detail == "not ours"
    ));
    assert!(c.is_locked());
    let c = client(TestKeyRoot::new(Mode::UnsealUnavailable));
    assert!(matches!(
        c.unlock(keys.clone(), "k.rao".into()),
        Err(ClientError::KeyRootUnavailable { detail }) if detail == "key deleted"
    ));
    assert!(c.is_locked());
}

#[test]
fn a_platform_side_that_panics_mid_prompt_leaves_a_client_that_recovers() {
    let root = TestKeyRoot::new(Mode::Honest);
    let c = client(root.clone());
    let keys = c.enrol().unwrap();
    root.panic.store(true, Ordering::SeqCst);
    let c2 = c.clone();
    let k2 = keys.clone();
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c2.unlock(k2, "k.rao".into())
    }));
    assert!(crashed.is_err());
    assert!(c.is_locked());
    assert_eq!(c.personas().len(), 4);
    root.panic.store(false, Ordering::SeqCst);
    // The source went with the crashed unlock: the next unlock reloads it.
    c.unlock(keys, "k.rao".into()).unwrap();
    assert!(!c
        .collection_rows("midgard.objectTypes".into())
        .unwrap()
        .is_empty());
}

#[test]
fn a_reset_during_the_prompt_wins() {
    let root = TestKeyRoot::new(Mode::Honest);
    let c = client(root.clone());
    let keys = c.enrol().unwrap();
    struct Resetter(Arc<TestKeyRoot>, OnceLock<Weak<MidgardClient>>);
    impl KeyRoot for Resetter {
        fn seal(&self, k: Vec<u8>) -> Result<Vec<u8>, KeyRootError> {
            self.0.seal(k)
        }
        fn unseal(&self, s: Vec<u8>) -> Result<Vec<u8>, KeyRootError> {
            if let Some(c) = self.1.get().and_then(Weak::upgrade) {
                c.reset_demo().unwrap();
            }
            self.0.unseal(s)
        }
        fn hardware_backed(&self) -> bool {
            false
        }
    }
    let r = Arc::new(Resetter(root, OnceLock::new()));
    let c2 = MidgardClient::new(config(&dataset()), r.clone()).unwrap();
    r.1.set(Arc::downgrade(&c2)).unwrap();
    assert!(matches!(
        c2.unlock(keys.clone(), "k.rao".into()),
        Err(ClientError::Locked)
    ));
    assert!(c2.is_locked());
    drop(c);
}
