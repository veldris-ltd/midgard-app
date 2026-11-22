//! The binding surface with no data source (`SourceChoice::None`): one `local`
//! persona, every action refused with "no data source connected" and nothing queued,
//! state round-trips, and a dataset path that does not open is an error, never a
//! fallback to this mode. [FR-C-1] [FR-C-5] [AC-C-13] [FR-C-4]

use std::path::PathBuf;
use std::sync::Arc;

use midgard_client_ffi::*;
use pqc_agility::{Aes256GcmAead, OsRng, Rng, GCM_NONCE_LEN};

/// A software platform key: AES-256-GCM under a fixed key.
struct TestKeyRoot;

const KEY: [u8; 32] = [9; 32];

impl KeyRoot for TestKeyRoot {
    fn seal(&self, data_key: Vec<u8>) -> Result<Vec<u8>, KeyRootError> {
        let mut nonce = [0u8; GCM_NONCE_LEN];
        OsRng.fill(&mut nonce);
        let ct = Aes256GcmAead::new(&KEY)
            .seal(&nonce, b"no-source-test", &data_key)
            .map_err(|e| KeyRootError::PlatformUnavailable {
                detail: e.to_string(),
            })?;
        Ok([nonce.to_vec(), ct].concat())
    }

    fn unseal(&self, sealed: Vec<u8>) -> Result<Vec<u8>, KeyRootError> {
        let (nonce, ct) = sealed.split_at(GCM_NONCE_LEN);
        Aes256GcmAead::new(&KEY)
            .open(nonce.try_into().unwrap(), b"no-source-test", ct)
            .map_err(|_| KeyRootError::Unseal)
    }

    fn hardware_backed(&self) -> bool {
        false
    }
}

fn config(dataset_path: Option<String>) -> ClientConfig {
    ClientConfig {
        source: match dataset_path {
            None => SourceChoice::None,
            Some(path) => SourceChoice::Bundled { path },
        },
        replica_id: "no-source-device".into(),
        device: DeviceInfo {
            ram_mb: 8192,
            has_accelerator: true,
        },
    }
}

fn local() -> Arc<MidgardClient> {
    MidgardClient::new(config(None), Arc::new(TestKeyRoot)).unwrap()
}

fn dataset() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../clients/demo/datasets")
        .join(format!("{}.mgdemo", core_version()))
}

fn ping() -> ActionInput {
    ActionInput {
        verb: "demo.ping".into(),
        target: "midgard.audit/AUD-1".into(),
        params: vec![],
    }
}

const REASON: &str = "no data source connected";

#[test]
fn no_source_lists_one_local_persona_and_no_data() {
    let c = local();
    assert_eq!(c.data_source_kind(), DataSourceKind::None);
    assert_eq!(
        c.personas(),
        vec![PersonaInfo {
            id: "local".into(),
            display: "Local user".into(),
            role: "operator".into(),
            clearance_label: "OPEN".into(),
            roles: vec![],
        }]
    );
    assert!(c.collections().is_empty());
    assert!(c.is_locked());
}

#[test]
fn local_signs_in_and_its_state_round_trips() {
    let c = local();
    let keys = c.enrol().unwrap();
    assert!(matches!(
        c.unlock(keys.clone(), "k.rao".into()),
        Err(ClientError::UnknownPersona { persona }) if persona == "k.rao"
    ));
    assert!(c.is_locked());
    let info = c.unlock(keys.clone(), "local".into()).unwrap();
    assert_eq!(info.persona.id, "local");
    assert_eq!(info.chips.sync.phase, SyncPhase::Synced);
    assert_eq!(info.chips.sync.pending, 0);
    assert_eq!(c.session_persona().unwrap().id, "local");
    assert_eq!(c.data_source_kind(), DataSourceKind::None);

    // The offline chip still works.
    assert_eq!(c.set_offline(true).unwrap().sync.phase, SyncPhase::Offline);
    assert_eq!(c.set_offline(false).unwrap().sync.phase, SyncPhase::Synced);
    let report = c.sync_now().unwrap();
    assert_eq!((report.pushed, report.merged), (0, 0));
    assert!(matches!(
        c.collection_rows("midgard.audit".into()),
        Err(ClientError::UnknownCollection { .. })
    ));

    let saved = c.export_state().unwrap();
    c.lock().unwrap();
    assert!(c.is_locked());

    // Relaunch: a fresh client, the same keys, the saved file.
    let c = local();
    c.unlock(keys, "local".into()).unwrap();
    let r = c.import_state(saved.file, saved.generation).unwrap();
    assert_eq!(r.generation, saved.generation);
    assert!(!r.pending_dropped);
    assert_eq!(c.status_chips().unwrap().sync.pending, 0);
    c.lock().unwrap();
}

#[test]
fn every_action_is_refused_and_nothing_is_queued() {
    let c = local();
    let keys = c.enrol().unwrap();
    assert!(matches!(c.request_action(ping()), Err(ClientError::Locked)));
    c.unlock(keys, "local".into()).unwrap();
    for offline in [false, true] {
        c.set_offline(offline).unwrap();
        match c.request_action(ping()) {
            Err(ClientError::Refused { reason }) => assert_eq!(reason, REASON),
            other => panic!("expected Refused, got {other:?}"),
        }
        // Even a target that is not `<collection>/<row id>` gets the same answer.
        assert!(matches!(
            c.request_action(ActionInput {
                verb: "x".into(),
                target: "no-slash".into(),
                params: vec![],
            }),
            Err(ClientError::Refused { .. })
        ));
        assert_eq!(c.status_chips().unwrap().sync.pending, 0);
    }
}

#[test]
fn reset_makes_a_fresh_local_source() {
    let c = local();
    let keys = c.enrol().unwrap();
    c.unlock(keys.clone(), "local".into()).unwrap();
    c.set_offline(true).unwrap();
    c.reset_demo().unwrap();
    assert!(c.is_locked());
    assert_eq!(c.data_source_kind(), DataSourceKind::None);
    let info = c.unlock(keys, "local".into()).unwrap();
    assert_eq!(
        info.chips.sync.phase,
        SyncPhase::Synced,
        "online again: a fresh source"
    );
}

#[test]
fn a_dataset_path_that_does_not_open_never_falls_back_to_local() {
    let dir = std::env::temp_dir().join(format!("midgard-ffi-nosrc-{}", std::process::id()));
    let missing = dir.join("absent.mgdemo").display().to_string();
    assert!(matches!(
        MidgardClient::new(config(Some(missing)), Arc::new(TestKeyRoot)),
        Err(ClientError::NotFound { .. })
    ));
    assert!(matches!(
        MidgardClient::new(config(Some(String::new())), Arc::new(TestKeyRoot)),
        Err(ClientError::NotFound { .. })
    ));

    let mut bytes = std::fs::read(dataset()).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 1;
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("tampered.mgdemo");
    std::fs::write(&bad, bytes).unwrap();
    let r = MidgardClient::new(
        config(Some(bad.display().to_string())),
        Arc::new(TestKeyRoot),
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(matches!(r, Err(ClientError::SignatureFailure { .. })));
}

#[test]
fn the_signed_dataset_still_opens_and_reports_demo() {
    let c = MidgardClient::new(
        config(Some(dataset().display().to_string())),
        Arc::new(TestKeyRoot),
    )
    .unwrap();
    assert_eq!(c.data_source_kind(), DataSourceKind::Demo);
    assert_eq!(c.personas().len(), 4);
    assert!(!c.personas().iter().any(|p| p.id == "local"));
    let keys = c.enrol().unwrap();
    assert!(matches!(
        c.unlock(keys.clone(), "local".into()),
        Err(ClientError::UnknownPersona { .. })
    ));
    c.unlock(keys, "k.rao".into()).unwrap();
    assert_eq!(c.data_source_kind(), DataSourceKind::Demo);
    c.reset_demo().unwrap();
    assert_eq!(c.data_source_kind(), DataSourceKind::Demo);
}
