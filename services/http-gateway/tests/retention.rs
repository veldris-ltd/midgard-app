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
