//! The binding surface over the release's sample data (`SourceChoice::Sample`): the
//! mock personas, every mock collection, the demonstration banner's kind, rows
//! clearance-filtered, Heimdall's authorise action as the web mock behaves, and a
//! reset back to the data as extracted. [FR-C-1] [FR-C-5] [AC-C-13] [FR-C-4]

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
            .seal(&nonce, b"sample-test", &data_key)
            .map_err(|e| KeyRootError::PlatformUnavailable {
                detail: e.to_string(),
            })?;
        Ok([nonce.to_vec(), ct].concat())
    }

    fn unseal(&self, sealed: Vec<u8>) -> Result<Vec<u8>, KeyRootError> {
        let (nonce, ct) = sealed.split_at(GCM_NONCE_LEN);
        Aes256GcmAead::new(&KEY)
            .open(nonce.try_into().unwrap(), b"sample-test", ct)
            .map_err(|_| KeyRootError::Unseal)
    }

    fn hardware_backed(&self) -> bool {
        false
    }
}

fn sample() -> Arc<MidgardClient> {
    let config = ClientConfig {
        source: SourceChoice::Sample,
        replica_id: "sample-device".into(),
        device: DeviceInfo {
            ram_mb: 8192,
            has_accelerator: true,
        },
    };
    MidgardClient::new(config, Arc::new(TestKeyRoot)).unwrap()
}

fn tracks(c: &MidgardClient) -> Vec<ConsoleRow> {
    c.collection_rows("heimdall.tracks".into()).unwrap()
}

#[test]
fn sample_lists_the_mock_personas_and_collections() {
    let c = sample();
    assert_eq!(c.data_source_kind(), DataSourceKind::Sample);
    let ids: Vec<String> = c.personas().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, ["k.rao", "j.benjamin", "k.osei", "a.holt"]);
    let names: Vec<String> = c.collections().into_iter().map(|x| x.name).collect();
    assert_eq!(names.len(), 53);
    assert!(names.contains(&"heimdall.tracks".to_string()));
    assert!(!names.contains(&"heimdall.motion".to_string()));
}

#[test]
fn rows_follow_the_personas_clearance() {
    let c = sample();
    let keys = c.enrol().unwrap();
    c.unlock(keys.clone(), "k.osei".into()).unwrap();
    let rows = tracks(&c);
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|r| !r.withheld));
    let Some(RowItem::HeimdallTrack { item }) = &rows[3].item else {
        panic!("not a track: {:?}", rows[3].item);
    };
    assert_eq!(
        (item.id.as_str(), item.track.as_str()),
        ("E-0871", "anomalous")
    );
    c.lock().unwrap();

    c.unlock(keys, "k.rao".into()).unwrap();
    let rows = tracks(&c);
    assert_eq!(rows.iter().filter(|r| r.withheld).count(), 4);
    assert!(rows
        .iter()
        .filter(|r| r.withheld)
        .all(|r| r.item.is_none() && r.id.is_none()));
}

#[test]
fn authorise_behaves_as_the_mock_and_reset_starts_again() {
    let c = sample();
    let keys = c.enrol().unwrap();
    c.unlock(keys.clone(), "k.osei".into()).unwrap();
    c.collection_rows("heimdall.workflows".into()).unwrap();
    let id = c
        .request_action(ActionInput {
            verb: "heimdall.authoriseWorkflow".into(),
            target: "heimdall.workflows/W-0912".into(),
            params: vec![KeyValue {
                key: "humanGateToken".into(),
                value: "HG-1".into(),
            }],
        })
        .unwrap();
    assert_eq!(
        c.action_status(id.clone()).unwrap().state,
        ActionState::Queued
    );
    c.sync_now().unwrap();
    let st = c.action_status(id).unwrap();
    assert_eq!(st.state, ActionState::Applied);
    assert_eq!(st.audit_id.as_deref(), Some("D-4473"));

    let gated = c
        .request_action(ActionInput {
            verb: "heimdall.authoriseWorkflow".into(),
            target: "heimdall.workflows/W-0913".into(),
            params: vec![],
        })
        .unwrap();
    c.sync_now().unwrap();
    let st = c.action_status(gated).unwrap();
    assert_eq!(st.state, ActionState::Refused);
    assert!(st.reason.unwrap().starts_with("Human gate required"));

    // Reset: the sample as extracted, so the next authorise is D-4473 again.
    c.reset_demo().unwrap();
    assert_eq!(c.data_source_kind(), DataSourceKind::Sample);
    let keys = c.enrol().unwrap();
    c.unlock(keys, "k.osei".into()).unwrap();
    c.collection_rows("heimdall.workflows".into()).unwrap();
    let id = c
        .request_action(ActionInput {
            verb: "heimdall.authoriseWorkflow".into(),
            target: "heimdall.workflows/W-0912".into(),
            params: vec![KeyValue {
                key: "humanGateToken".into(),
                value: "HG-2".into(),
            }],
        })
        .unwrap();
    c.sync_now().unwrap();
    assert_eq!(
        c.action_status(id).unwrap().audit_id.as_deref(),
        Some("D-4473")
    );
}
