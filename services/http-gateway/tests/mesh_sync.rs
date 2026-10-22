//! Syncing records into a real enclosure through the mesh door: a permitted workload's
//! sync creates objects and links, and syncing again updates objects without adding a
//! link twice; a workload policy does not permit, records the ontology does not declare
//! and a link to an object that does not exist are refused and change nothing; every
//! sync is audited; and what was synced is durable. [FR-F-4] [NFR-SEC-1] [AC-M-4]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use attest::Keystore;
use fabric::{DurableObjectStore, ObjectStore, Value};
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
                      { "name": "flight_hours", "type": "int" } ] },
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
    std::env::temp_dir().join(format!("mesh-sync-{tag}-{}-{}", std::process::id(), uniq))
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

fn aircraft(key: &str, hours: i64) -> String {
    format!(
        r#"{{"object":"aircraft","key":"{key}","marking":"internal","purposes":["operations"],"properties":{{"tail":"{key}","flight_hours":{hours}}}}}"#
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
fn a_permitted_workload_syncs_records_and_nothing_else_changes_the_fabric() {
    let dir = scratch("sync");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    let analyst = workload("analyst-tool", "analyst");

    {
        let (gateway, _lock) = open_state_dir(&state, Vec::new()).unwrap().into_parts();
        let sync = |peer: &WorkloadIdentity, records: &str| {
            split(&gateway.sync_records(peer, PURPOSE, records.as_bytes(), NOW))
        };

        let first = [
            aircraft("AC-1", 100),
            aircraft("AC-2", 200),
            FACILITY.to_string(),
            link("AC-1"),
            link("AC-2"),
        ]
        .join("\n");
        let (code, message) = sync(&worker, &first);
        assert_eq!(code, outcome::ACCEPTED, "{message}");
        assert!(
            message.contains("3 created, 0 updated, 2 link(s) added"),
            "{message}"
        );

        let second = [
            aircraft("AC-1", 150),
            aircraft("AC-2", 200),
            FACILITY.to_string(),
            link("AC-1"),
            link("AC-2"),
        ]
        .join("\n");
        let (code, message) = sync(&worker, &second);
        assert_eq!(code, outcome::ACCEPTED, "{message}");
        assert!(
            message.contains("0 created, 3 updated, 0 link(s) added"),
            "{message}"
        );

        let (code, _) = sync(&analyst, &aircraft("AC-3", 1));
        assert_eq!(code, outcome::DENIED, "policy does not permit an analyst");

        let submarine =
            r#"{"object":"submarine","key":"SS-1","marking":"open","purposes":["operations"]}"#;
        let (code, message) = sync(&worker, submarine);
        assert_eq!(code, outcome::MALFORMED);
        assert!(message.contains("submarine"), "{message}");

        let (code, message) = sync(&worker, &link("AC-9"));
        assert_eq!(code, outcome::MALFORMED);
        assert!(message.contains("aircraft/AC-9"), "{message}");
    }

    // Durable: the update landed, and the link was not added twice.
    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    {
        let store = DurableObjectStore::open(state.store(), Box::new(sealer), STORE_ID).unwrap();
        assert_eq!(
            store
                .get_object("aircraft/AC-1")
                .and_then(|o| o.properties.get("flight_hours")),
            Some(&Value::Int(150))
        );
        assert!(store.get_object("aircraft/AC-3").is_none());
        assert!(store.get_object("submarine/SS-1").is_none());
        assert_eq!(store.neighbors("aircraft/AC-1", "aircraft_base").len(), 1);
    }

    // Audited: each sync's decision, and each applied sync's record. Whatever a boot
    // writes itself is the same on every boot, so it cancels out.
    let after_syncs = audit_entries(&state);
    let per_boot = audit_entries(&state) - after_syncs;
    assert_eq!(
        after_syncs - per_boot,
        7,
        "two applied syncs (decision and record each) and three refused decisions"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sync_is_flushed_to_disk_once_however_many_records_it_holds() {
    let dir = scratch("one-flush");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    let keys: Vec<String> = (0..200).map(|i| format!("AC-{i}")).collect();
    {
        let (gateway, _lock) = open_state_dir(&state, Vec::new()).unwrap().into_parts();
        let mut records: Vec<String> = keys.iter().map(|k| aircraft(k, 1)).collect();
        records.push(FACILITY.to_string());
        records.extend(keys.iter().map(|k| link(k)));
        let before = gateway.store_flushes();
        let (code, message) =
            split(&gateway.sync_records(&worker, PURPOSE, records.join("\n").as_bytes(), NOW));
        assert_eq!(code, outcome::ACCEPTED, "{message}");
        assert!(
            message.contains("201 created, 0 updated, 200 link(s) added"),
            "{message}"
        );
        assert_eq!(
            gateway.store_flushes(),
            before + 1,
            "one flush for 401 records"
        );
    }
    // Acknowledged, so on disk: every record is there after a restart.
    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let store = DurableObjectStore::open(state.store(), Box::new(sealer), STORE_ID).unwrap();
    assert_eq!(store.objects_of_type("aircraft").len(), 200);
    assert_eq!(store.neighbors("aircraft/AC-199", "aircraft_base").len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
