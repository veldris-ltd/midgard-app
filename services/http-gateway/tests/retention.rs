//! Retention windows enforced on a real enclosure: a sweep removes only the objects of
//! a type with a window whose latest named time is older than it, with their links,
//! records the sweep in the ledger before removing anything, and the removal survives
//! a restart. Objects with no known age, and types without a window, are kept.
//! [SAD_00 §8.5] [NFR-SEC-2] [AC-M-4]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use attest::Keystore;
use fabric::{DurableObjectStore, ObjectStore};
use http_gateway::boot::open_state_dir;
use http_gateway::mesh_sync::outcome;
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, AUDIT_KEY_ID, IDP_KEY_ID, STORE_ID};
use kms::{Kms, KmsSealer};
use policy::{Action, Effect, PolicySet, Role, Rule, RuleId, Selector};
use pq_mtls::{ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

const NOW: u64 = 1_800_000_000;
const PURPOSE: &str = "data-integration";

const ONTOLOGY: &str = r#"{
  "version": 1,
  "objectTypes": [
    { "id": "aircraft", "title": "Aircraft",
      "properties": [ { "name": "tail", "type": "text", "required": true },
                      { "name": "flight_hours", "type": "int" },
                      { "name": "seen", "type": "timestamp" } ],
      "retention": { "after": ["seen"], "hours": 1 } },
    { "id": "facility", "title": "Facility",
      "properties": [ { "name": "name", "type": "text", "required": true } ] }
  ],
  "linkTypes": [
    { "id": "aircraft_base", "name": "based at", "from": "aircraft", "to": "facility",
      "cardinality": "many-to-many" }
  ],
  "actions": []
}"#;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    std::env::temp_dir().join(format!("retention-{tag}-{}-{}", std::process::id(), uniq))
}

fn provision(dir: &Path) -> StateDir {
    let state = StateDir::new(dir);
    std::fs::create_dir_all(state.keys()).unwrap();
    std::fs::create_dir_all(state.trust()).unwrap();
    Keystore::generate(IDP_KEY_ID, iam::IDP_SIG_ALG, &mut OsRng)
        .unwrap()
        .write(&state.idp_keystore())
        .unwrap();
    Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut OsRng)
        .unwrap()
        .write(&state.audit_keystore())
        .unwrap();
    let rule = |id: &str, role: &str, action: &str| Rule {
        id: RuleId(id.into()),
        role: Role(role.into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    };
    let policy = PolicySet::new(vec![
        rule("worker-sync", "forge-worker", "SyncRecords"),
        rule("operator-read", "operator", "Read"),
    ])
    .unwrap();
    std::fs::write(state.policy(), render_policy(&policy).unwrap()).unwrap();
    std::fs::write(state.ontology(), ONTOLOGY).unwrap();
    state
}

fn workload(name: &str, role: &str) -> WorkloadIdentity {
    WorkloadIdentity {
        id: ServiceId(name.into()),
        roles: [Role(role.into())].into_iter().collect(),
        sig_alg: iam::IDP_SIG_ALG,
        verifying_key: Keystore::generate(name, iam::IDP_SIG_ALG, &mut OsRng)
            .unwrap()
            .verifying_key,
    }
}

fn split(response: &[u8]) -> (u8, String) {
    (
        response[0],
        String::from_utf8_lossy(&response[1..]).into_owned(),
    )
}

fn aircraft(key: &str, seen: Option<u64>) -> String {
    let seen = seen.map(|s| format!(r#","seen":{s}"#)).unwrap_or_default();
    format!(
        r#"{{"object":"aircraft","key":"{key}","marking":"internal","purposes":["operations"],"properties":{{"tail":"{key}"{seen}}}}}"#
    )
}

const FACILITY: &str = r#"{"object":"facility","key":"FAC-N","marking":"internal","purposes":["operations"],"properties":{"name":"Northern base"}}"#;

fn link(from: &str) -> String {
    format!(r#"{{"link":"aircraft_base","from":"{from}","to":"FAC-N"}}"#)
}

fn audit_entries(state: &StateDir) -> usize {
    open_state_dir(state, Vec::new()).unwrap().audit_entries
}

#[test]
fn a_sweep_removes_expired_objects_and_their_links_and_is_audited_first() {
    let dir = scratch("sweep");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    {
        let (gateway, _lock) = open_state_dir(&state, Vec::new()).unwrap().into_parts();
        assert!(gateway.has_retention());
        let records = [
            aircraft("AC-OLD", Some(NOW - 7200)),
            aircraft("AC-NEW", Some(NOW - 60)),
            aircraft("AC-AGELESS", None),
            FACILITY.to_string(),
            link("AC-OLD"),
            link("AC-NEW"),
        ]
        .join("\n");
        let (code, message) =
            split(&gateway.sync_records(&worker, PURPOSE, records.as_bytes(), NOW));
        assert_eq!(code, outcome::ACCEPTED, "{message}");
    }
    let before = audit_entries(&state);
    {
        let (gateway, _lock) = open_state_dir(&state, Vec::new()).unwrap().into_parts();
        let swept = gateway.enforce_retention(NOW).unwrap();
        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].object_type, "aircraft");
        assert_eq!(swept[0].removed, 1);
        assert!(!swept[0].more);
        assert!(
            gateway.enforce_retention(NOW).unwrap().is_empty(),
            "nothing left to expire"
        );
    }
    assert_eq!(
        audit_entries(&state),
        before + 1,
        "one ledger entry for the one sweep"
    );

    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let store = DurableObjectStore::open(state.store(), Box::new(sealer), STORE_ID).unwrap();
    assert!(
        store.get_object("aircraft/AC-OLD").is_none(),
        "removed, durably"
    );
    assert!(store.get_object("aircraft/AC-NEW").is_some());
    assert!(
        store.get_object("aircraft/AC-AGELESS").is_some(),
        "no known age: kept"
    );
    assert!(
        store.get_object("facility/FAC-N").is_some(),
        "no window: kept"
    );
    assert!(store
        .neighbors("aircraft/AC-OLD", "aircraft_base")
        .is_empty());
    assert_eq!(store.neighbors("aircraft/AC-NEW", "aircraft_base").len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_outgrown_journal_is_compacted_audited_and_reopens_identically() {
    let dir = scratch("compact");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    let records = [
        aircraft("AC-1", Some(NOW)),
        FACILITY.to_string(),
        link("AC-1"),
    ]
    .join("\n");
    {
        let (gateway, _lock) = open_state_dir(&state, Vec::new()).unwrap().into_parts();
        let sync = |r: &str| split(&gateway.sync_records(&worker, PURPOSE, r.as_bytes(), NOW));
        assert_eq!(sync(&records).0, outcome::ACCEPTED);
        assert_eq!(
            gateway.compact_store_if_due(NOW).unwrap(),
            None,
            "not outgrown yet"
        );
        // 50 aircraft updated 10 times: far more records than data.
        let update = (0..50)
            .map(|i| aircraft(&format!("AC-{i}"), Some(NOW)))
            .collect::<Vec<_>>()
            .join("\n");
        for _ in 0..10 {
            assert_eq!(sync(&update).0, outcome::ACCEPTED);
        }
    }
    let before = audit_entries(&state);
    {
        let (gateway, _lock) = open_state_dir(&state, Vec::new()).unwrap().into_parts();
        assert_eq!(
            gateway.compact_store_if_due(NOW).unwrap(),
            None,
            "within the slack"
        );
        let (was, now) = gateway
            .compact_store_with_slack(NOW, 100)
            .unwrap()
            .expect("outgrown");
        // AC-1 was already there; the others were created by the first round.
        assert_eq!((was, now), (3 + 10 * 50, 50 + 1 + 1));
        assert_eq!(gateway.compact_store_with_slack(NOW, 100).unwrap(), None);
    }
    assert_eq!(
        audit_entries(&state),
        before + 1,
        "the compaction is audited"
    );
    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let store = DurableObjectStore::open(state.store(), Box::new(sealer), STORE_ID).unwrap();
    assert!(store.get_object("aircraft/AC-1").is_some());
    assert_eq!(store.neighbors("aircraft/AC-1", "aircraft_base").len(), 1);
    assert_eq!(store.journal_records(), 52);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_feed_refresh_keeps_what_actions_own() {
    let dir = scratch("owned");
    let state = provision(&dir);
    // An ontology whose aircraft carry an action-owned status.
    let ontology = ONTOLOGY
        .replace(
            r#"{ "name": "seen", "type": "timestamp" } ],"#,
            r#"{ "name": "seen", "type": "timestamp" },
                      { "name": "status", "type": "text" } ],"#,
        )
        .replace(
            r#""actions": []"#,
            r#""actions": [ { "id": "ground", "name": "ground", "title": "Ground",
                "objectType": "aircraft", "marking": "internal", "purposes": ["operations"],
                "effect": { "kind": "set-property", "property": "status", "value": "grounded" } } ]"#,
        );
    assert_ne!(ontology, ONTOLOGY);
    std::fs::write(state.ontology(), ontology).unwrap();
    let worker = workload("forge-worker", "forge-worker");
    let with_status = r#"{"object":"aircraft","key":"AC-1","marking":"internal","purposes":["operations"],"properties":{"tail":"AC-1","status":"grounded","flight_hours":1}}"#;
    let without = r#"{"object":"aircraft","key":"AC-1","marking":"internal","purposes":["operations"],"properties":{"tail":"AC-1","flight_hours":2}}"#;
    {
        let (gateway, _lock) = open_state_dir(&state, Vec::new()).unwrap().into_parts();
        for records in [with_status, without] {
            let (code, message) =
                split(&gateway.sync_records(&worker, PURPOSE, records.as_bytes(), NOW));
            assert_eq!(code, outcome::ACCEPTED, "{message}");
        }
    }
    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let store = DurableObjectStore::open(state.store(), Box::new(sealer), STORE_ID).unwrap();
    let props = &store.get_object("aircraft/AC-1").unwrap().properties;
    assert_eq!(
        props.get("status"),
        Some(&fabric::Value::Text("grounded".into())),
        "kept"
    );
    assert_eq!(
        props.get("flight_hours"),
        Some(&fabric::Value::Int(2)),
        "refreshed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sweep_is_flushed_to_disk_once_however_many_objects_it_removes() {
    let dir = scratch("sweep-one-flush");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    let (gateway, _lock) = open_state_dir(&state, Vec::new()).unwrap().into_parts();
    let records: Vec<String> = (0..100)
        .map(|i| aircraft(&format!("AC-{i}"), Some(NOW - 7200)))
        .collect();
    let (code, message) =
        split(&gateway.sync_records(&worker, PURPOSE, records.join("\n").as_bytes(), NOW));
    assert_eq!(code, outcome::ACCEPTED, "{message}");
    let before = gateway.store_flushes();
    let swept = gateway.enforce_retention(NOW).unwrap();
    assert_eq!(swept[0].removed, 100);
    assert_eq!(
        gateway.store_flushes(),
        before + 1,
        "one flush for 100 removals"
    );
    drop(gateway);
    drop(_lock);
    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let store = DurableObjectStore::open(state.store(), Box::new(sealer), STORE_ID).unwrap();
    assert!(
        store.objects_of_type("aircraft").is_empty(),
        "removed, durably"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
