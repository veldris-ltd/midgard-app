//! Syncing records into a real enclosure through the mesh door: a permitted workload's
//! sync creates objects and links, and syncing again updates objects without adding a
//! link twice; a workload policy does not permit, records the ontology does not declare
//! and a link to an object that does not exist are refused and change nothing; every
//! sync is audited; and what was synced is durable. A sync may raise but never lower an
//! object's marking, never widen its purposes, never write above the gateway's sync
//! clearance and never touch an object under a human gate; and a workload policy does
//! not permit learns nothing about the ontology. [FR-F-4] [NFR-SEC-1] [NFR-SEC-2]
//! [NFR-AI-1] [AC-M-4] [VLD-ANB-MIDGARD-0001] [VLD-ANB-MIDGARD-0073]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use attest::Keystore;
use audit_writer::{FileLedger, LedgerId, LedgerStore};
use fabric::{DurableObjectStore, ObjectInstance, ObjectStore, Value};
use http_gateway::boot::{open_state_dir, DurableGateway};
use http_gateway::cli::parse_sync_clearance;
use http_gateway::http::Request;
use http_gateway::json::parse;
use http_gateway::mesh_sync::outcome;
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, StateLock, AUDIT_KEY_ID, IDP_KEY_ID, LEDGER_ID, STORE_ID};
use iam::UserRecord;
use kms::{Kms, KmsSealer};
use policy::{
    Action, ClassificationLevel, Clearance, Compartment, Effect, PolicySet, Purpose, Role, Rule,
    RuleId, Selector,
};
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
                      { "name": "status", "type": "text" } ] },
    { "id": "facility", "title": "Facility",
      "properties": [ { "name": "name", "type": "text", "required": true } ] }
  ],
  "linkTypes": [
    { "id": "aircraft_base", "name": "based at", "from": "aircraft", "to": "facility",
      "cardinality": "many-to-many" }
  ],
  "actions": [
    { "id": "ground_aircraft", "name": "groundAircraft", "title": "Ground aircraft",
      "objectType": "aircraft", "marking": "internal", "purposes": ["operations"],
      "humanGated": true,
      "effect": { "kind": "set-property", "property": "status", "value": "grounded" } }
  ]
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
    // What `mg-admin init` also creates: the ledger and its signed checkpoint.
    http_gateway::boot::init_audit_ledger(&state).unwrap();
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
        rule("operator-apply", "operator", "Apply"),
        rule("operator-ground", "operator", "groundAircraft"),
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
    open_state_dir(state, Vec::new(), &pin(state))
        .unwrap()
        .audit_entries
}

#[test]
fn a_permitted_workload_syncs_records_and_nothing_else_changes_the_fabric() {
    let dir = scratch("sync");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    let analyst = workload("analyst-tool", "analyst");

    {
        let (gateway, _lock) = open_state_dir(&state, Vec::new(), &pin(&state))
            .unwrap()
            .into_parts();
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

    // Audited: each sync's decision, each applied sync's record, and the malformed
    // sync's record. Whatever a boot writes itself is the same on every boot, so it
    // cancels out. [VLD-ANB-MIDGARD-0073]
    let after_syncs = audit_entries(&state);
    let per_boot = audit_entries(&state) - after_syncs;
    assert_eq!(
        after_syncs - per_boot,
        8,
        "two applied syncs (decision and record each), three refused decisions and one \
         malformed sync's record"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The audit key the operator pins the gateway to, from outside the keystore it
/// checks. [MG-SEC-05]
fn pin(state: &StateDir) -> audit_writer::PinnedKey {
    audit_writer::PinnedKey::new(
        attest::Keystore::read(&state.audit_keystore())
            .unwrap()
            .verifying_key,
    )
}

/// An aircraft record with an explicit marking, compartments and purposes.
fn marked(key: &str, band: &str, compartments: &[&str], purposes: &[&str]) -> String {
    let list = |items: &[&str]| {
        items
            .iter()
            .map(|i| format!("\"{i}\""))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        r#"{{"object":"aircraft","key":"{key}","marking":"{band}","compartments":[{}],"purposes":[{}],"properties":{{"tail":"{key}"}}}}"#,
        list(compartments),
        list(purposes)
    )
}

fn level(band: &str) -> ClassificationLevel {
    http_gateway::enclosure::level_of(band).unwrap()
}

/// Every governance ledger row, as (action, resource), read from the file the gateway
/// writes.
fn ledger_rows(state: &StateDir) -> Vec<(String, String)> {
    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let ledger =
        FileLedger::open(state.ledger(), Box::new(sealer), LedgerId(LEDGER_ID.into())).unwrap();
    ledger
        .entries()
        .iter()
        .map(|e| (e.record.action.0.clone(), e.record.resource.0.clone()))
        .collect()
}

/// The ledger rows written since it held `before`.
fn rows_since(state: &StateDir, before: usize) -> Vec<(String, String)> {
    ledger_rows(state).split_off(before)
}

fn row(action: &str, resource: &str) -> (String, String) {
    (action.to_string(), resource.to_string())
}

/// Object `id` as the durable store holds it, once `gateway` has shut down.
fn stored(
    state: &StateDir,
    gateway: DurableGateway,
    lock: StateLock,
    id: &str,
) -> Option<ObjectInstance> {
    drop((gateway, lock));
    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let store = DurableObjectStore::open(state.store(), Box::new(sealer), STORE_ID).unwrap();
    store.get_object(id).cloned()
}

/// A sync that would lower an object's marking is refused whole: nothing in it is
/// written, and the refusal is audited against the object. [NFR-SEC-2]
/// [VLD-ANB-MIDGARD-0001] [AC-18.1]
#[test]
fn a_sync_that_lowers_a_marking_is_refused_and_changes_nothing() {
    let dir = scratch("downgrade");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    let (gateway, lock) = open_state_dir(&state, Vec::new(), &pin(&state))
        .unwrap()
        .into_parts();
    let sync =
        |records: &str| split(&gateway.sync_records(&worker, PURPOSE, records.as_bytes(), NOW));

    let (code, message) = sync(&marked("AC-1", "confidential", &[], &["operations"]));
    assert_eq!(code, outcome::ACCEPTED, "{message}");

    let before = ledger_rows(&state).len();
    let batch = [
        marked("AC-1", "open", &[], &["operations"]),
        marked("AC-2", "open", &[], &["operations"]),
    ]
    .join("\n");
    let (code, message) = sync(&batch);
    assert_eq!(code, outcome::DENIED, "{message}");
    assert!(
        message.contains("aircraft/AC-1") && message.contains("marking-downgrade"),
        "{message}"
    );
    assert_eq!(
        rows_since(&state, before),
        vec![
            row("PERMIT SyncRecords", "fabric-sync"),
            row("SYNC refused: marking-downgrade", "object:aircraft/AC-1"),
        ],
        "the decision and the refusal, and no record of the batch"
    );

    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    drop((gateway, lock));
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let store = DurableObjectStore::open(state.store(), Box::new(sealer), STORE_ID).unwrap();
    assert_eq!(
        store.get_object("aircraft/AC-1").map(|o| o.marking.level),
        Some(ClassificationLevel(3)),
        "the stored marking is unchanged"
    );
    assert!(
        store.get_object("aircraft/AC-2").is_none(),
        "nothing in the batch is written"
    );
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A sync may raise an object's marking and narrow its purposes, but not widen them.
/// [NFR-SEC-2] [VLD-ANB-MIDGARD-0001] [AC-18.2]
#[test]
fn a_sync_may_raise_a_marking_and_narrow_purposes() {
    let dir = scratch("narrow");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    let (gateway, lock) = open_state_dir(&state, Vec::new(), &pin(&state))
        .unwrap()
        .into_parts();
    let sync =
        |records: &str| split(&gateway.sync_records(&worker, PURPOSE, records.as_bytes(), NOW));

    let (code, message) = sync(&marked(
        "AC-1",
        "internal",
        &[],
        &["operations", "analysis"],
    ));
    assert_eq!(code, outcome::ACCEPTED, "{message}");
    let (code, message) = sync(&marked("AC-1", "restricted", &[], &["operations"]));
    assert_eq!(code, outcome::ACCEPTED, "raised and narrowed: {message}");
    assert!(message.contains("0 created, 1 updated"), "{message}");

    let (code, message) = sync(&marked(
        "AC-1",
        "restricted",
        &[],
        &["operations", "export"],
    ));
    assert_eq!(code, outcome::DENIED, "{message}");
    assert!(message.contains("purpose-widening"), "{message}");

    let object = stored(&state, gateway, lock, "aircraft/AC-1").unwrap();
    assert_eq!(object.marking.level, level("restricted"));
    assert_eq!(
        object.allowed_purposes,
        [Purpose("operations".into())].into_iter().collect()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// By default a sync writes any band but no compartment; a compartment needs an
/// explicit grant of the sync clearance. [NFR-SEC-2] [VLD-ANB-MIDGARD-0001] [AC-18.3]
#[test]
fn a_sync_above_the_sync_clearance_is_refused() {
    let dir = scratch("ceiling");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    let record = marked("AC-1", "confidential", &["OPS-N"], &["operations"]);

    {
        let (gateway, _lock) = open_state_dir(&state, Vec::new(), &pin(&state))
            .unwrap()
            .into_parts();
        let before = ledger_rows(&state).len();
        let (code, message) =
            split(&gateway.sync_records(&worker, PURPOSE, record.as_bytes(), NOW));
        assert_eq!(code, outcome::DENIED, "{message}");
        assert!(
            message.contains("marking-above-sync-clearance"),
            "{message}"
        );
        assert_eq!(
            rows_since(&state, before).last(),
            Some(&row(
                "SYNC refused: marking-above-sync-clearance",
                "object:aircraft/AC-1"
            ))
        );
    }

    // What `--mesh-sync-clearance confidential:OPS-N` grants.
    let granted = parse_sync_clearance("confidential:OPS-N").unwrap();
    assert_eq!(
        granted,
        Clearance {
            level: ClassificationLevel::TOP_SECRET,
            compartments: [Compartment("OPS-N".into())].into_iter().collect(),
        }
    );
    assert!(
        parse_sync_clearance("secret").is_err(),
        "not a marking band"
    );
    let (gateway, lock) = open_state_dir(&state, Vec::new(), &pin(&state))
        .unwrap()
        .into_parts();
    let gateway = gateway.with_sync_clearance(granted);
    let (code, message) = split(&gateway.sync_records(&worker, PURPOSE, record.as_bytes(), NOW));
    assert_eq!(code, outcome::ACCEPTED, "{message}");
    drop((gateway, lock));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A workload policy does not permit is refused before its records are read, so the
/// reply carries no ontology detail, and only the decision is audited.
/// [NFR-SEC-1] [VLD-ANB-MIDGARD-0073] [AC-18.5]
#[test]
fn an_unpermitted_peer_learns_nothing_from_a_malformed_sync() {
    let dir = scratch("oracle");
    let state = provision(&dir);
    let analyst = workload("analyst-tool", "analyst");
    let (gateway, lock) = open_state_dir(&state, Vec::new(), &pin(&state))
        .unwrap()
        .into_parts();

    let before = ledger_rows(&state).len();
    let (code, message) =
        split(&gateway.sync_records(&analyst, PURPOSE, SUBMARINE.as_bytes(), NOW));
    assert_eq!(code, outcome::DENIED, "{message}");
    assert!(!message.contains("submarine"), "{message}");
    assert!(!message.contains("ontology"), "{message}");
    assert_eq!(
        rows_since(&state, before),
        vec![row("DENY SyncRecords", "fabric-sync")]
    );
    drop((gateway, lock));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A record of a type the test ontology does not declare.
const SUBMARINE: &str =
    r#"{"object":"submarine","key":"SS-1","marking":"open","purposes":["operations"]}"#;

/// A malformed sync from a permitted workload is audited as well as refused.
/// [NFR-SEC-1] [VLD-ANB-MIDGARD-0073] [AC-18.5]
#[test]
fn a_malformed_sync_from_a_permitted_peer_is_still_audited() {
    let dir = scratch("malformed");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    let (gateway, lock) = open_state_dir(&state, Vec::new(), &pin(&state))
        .unwrap()
        .into_parts();

    let before = ledger_rows(&state).len();
    let (code, message) = split(&gateway.sync_records(&worker, PURPOSE, SUBMARINE.as_bytes(), NOW));
    assert_eq!(code, outcome::MALFORMED, "{message}");
    assert!(message.contains("submarine"), "{message}");
    assert_eq!(
        rows_since(&state, before),
        vec![
            row("PERMIT SyncRecords", "fabric-sync"),
            row("SYNC records: MALFORMED", "sync"),
        ]
    );
    drop((gateway, lock));
    let _ = std::fs::remove_dir_all(&dir);
}

/// An object a named human has been asked to approve a change to cannot be overwritten
/// by a sync. [NFR-AI-1] [VLD-ANB-MIDGARD-0001] [AC-18.4]
#[test]
fn a_sync_never_touches_a_human_gated_target() {
    const PASSWORD: &str = "correct horse battery staple";
    let dir = scratch("gated");
    let state = provision(&dir);
    let worker = workload("forge-worker", "forge-worker");
    let (mut gateway, lock) = open_state_dir(&state, Vec::new(), &pin(&state))
        .unwrap()
        .into_parts();
    gateway
        .provision_login(
            "olivia",
            PASSWORD,
            UserRecord {
                roles: [Role("operator".into())].into_iter().collect(),
                clearance: Clearance {
                    level: level("internal"),
                    compartments: Default::default(),
                },
                purposes: [Purpose("operations".into())].into_iter().collect(),
            },
        )
        .unwrap();
    let call = |gateway: &DurableGateway, path: &str, body: &str, token: Option<&str>| {
        gateway.handle(
            &Request {
                method: "POST".into(),
                path: path.into(),
                query: String::new(),
                body: body.into(),
                authorization: token.map(|t| format!("Bearer {t}")),
                origin: None,
            },
            NOW,
        )
    };

    let (code, message) =
        split(&gateway.sync_records(&worker, PURPOSE, aircraft("AC-1", 100).as_bytes(), NOW));
    assert_eq!(code, outcome::ACCEPTED, "{message}");

    let login = call(
        &gateway,
        "/api/v1/login",
        &format!(r#"{{"username":"olivia","password":"{PASSWORD}"}}"#),
        None,
    );
    assert_eq!(login.status, 200, "{}", login.body);
    let token = parse(&login.body)
        .unwrap()
        .str_field("token")
        .expect("token")
        .to_string();
    let requested = call(
        &gateway,
        "/api/v1/approvals",
        r#"{"actionType":"Ground aircraft","target":"aircraft/AC-1"}"#,
        Some(&token),
    );
    assert_eq!(requested.status, 200, "{}", requested.body);

    let before = ledger_rows(&state).len();
    let (code, message) =
        split(&gateway.sync_records(&worker, PURPOSE, aircraft("AC-1", 999).as_bytes(), NOW));
    assert_eq!(code, outcome::DENIED, "{message}");
    assert!(message.contains("human-gated-target"), "{message}");
    assert_eq!(
        rows_since(&state, before),
        vec![
            row("PERMIT SyncRecords", "fabric-sync"),
            row("SYNC refused: human-gated-target", "object:aircraft/AC-1"),
        ]
    );

    let object = stored(&state, gateway, lock, "aircraft/AC-1").unwrap();
    assert_eq!(
        object.properties.get("flight_hours"),
        Some(&Value::Int(100)),
        "the object is unchanged"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
