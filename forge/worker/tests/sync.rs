//! `forge-worker` end to end, against a real enclosure: the real worker binary syncs two
//! CSV sources into its sealed dataset store and, over PQ-mTLS through the gateway's
//! mesh door, into the enclosure's fabric — facilities first, then aircraft linked to
//! them. Running again leaves the fabric as it was; a changed source reaches the
//! fabric; every version is kept; and a workload policy does not permit to sync is
//! refused. [FR-F-2] [FR-F-3] [FR-F-4] [RC-11] [NFR-SEC-1]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use attest::Keystore;
use fabric::{DurableObjectStore, ObjectStore, Value};
use forge_dataset_store::DurableDatasetStore;
use http_gateway::boot::open_state_dir;
use http_gateway::mesh_sync::spawn_mesh_listener;
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, AUDIT_KEY_ID, IDP_KEY_ID, STORE_ID};
use iam::{Idp, IDP_SIG_ALG};
use kms::{Kms, KmsSealer};
use policy::{Action, DefaultPdp, Effect, PolicySet, Role, Rule, RuleId, Selector};
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;
use service_runtime::ServiceIdentity;

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

const JOBS: &str = r#"{
  "version": 1,
  "jobs": [
    { "id": "facilities", "every": 300,
      "source": { "file": "facilities.csv",
                  "columns": [ { "name": "key", "type": "text" },
                               { "name": "name", "type": "text" } ] },
      "objects": { "type": "facility", "key": "key", "marking": "internal",
                   "purposes": ["operations"], "properties": { "name": "name" } } },
    { "id": "aircraft", "every": 60,
      "source": { "file": "aircraft.csv",
                  "columns": [ { "name": "tail", "type": "text" },
                               { "name": "hours", "type": "int", "nullable": true },
                               { "name": "base", "type": "text" } ] },
      "objects": { "type": "aircraft", "key": "tail", "marking": "internal",
                   "purposes": ["operations"],
                   "properties": { "tail": "tail", "flight_hours": "hours" } },
      "links": [ { "type": "aircraft_base", "from": "tail", "to": "base" } ] }
  ]
}"#;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir =
        std::env::temp_dir().join(format!("forge-worker-{tag}-{}-{uniq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An enclosure state directory whose policy lets the worker role open the mesh
/// channel and sync, and lets an analyst open the channel only.
fn provision(dir: &Path) -> StateDir {
    let state = StateDir::new(dir);
    std::fs::create_dir_all(state.keys()).unwrap();
    std::fs::create_dir_all(state.trust()).unwrap();
    Keystore::generate(IDP_KEY_ID, IDP_SIG_ALG, &mut OsRng)
        .unwrap()
        .write(&state.idp_keystore())
        .unwrap();
    Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut OsRng)
        .unwrap()
        .write(&state.audit_keystore())
        .unwrap();
    std::fs::write(state.policy(), render_policy(&policy()).unwrap()).unwrap();
    std::fs::write(state.ontology(), ONTOLOGY).unwrap();
    state
}

fn policy() -> PolicySet {
    let rule = |id: &str, role: &str, action: &str| Rule {
        id: RuleId(id.into()),
        role: Role(role.into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    };
    PolicySet::new(vec![
        rule("worker-channel", "forge-worker", "OpenChannel"),
        rule("worker-sync", "forge-worker", "SyncRecords"),
        rule("analyst-channel", "analyst", "OpenChannel"),
    ])
    .unwrap()
}

/// What `mg-admin mesh issue` writes, for `service` under `idp`.
fn issue(idp: &Idp, dir: &Path, service: &str, roles: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    let key = Keystore::generate(service, IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(&format!("{service}-audit"), IDP_SIG_ALG, &mut OsRng).unwrap();
    let cert = idp
        .certify(
            &WorkloadIdentity {
                id: ServiceId(service.into()),
                roles: roles.iter().map(|r| Role(r.to_string())).collect(),
                sig_alg: IDP_SIG_ALG,
                verifying_key: key.verifying_key.clone(),
            },
            pq_mtls::CertTerms::new(
                1,
                0,
                u64::MAX,
                pq_mtls::Posture::at(policy::ClassificationLevel::TOP_SECRET),
            ),
        )
        .unwrap();
    key.write(&dir.join(format!("{service}.keystore"))).unwrap();
    audit
        .write(&dir.join(format!("{service}.audit.keystore")))
        .unwrap();
    std::fs::write(dir.join(format!("{service}.cert")), encode_cert(&cert)).unwrap();
    std::fs::write(dir.join("mesh.anchor"), encode_anchor(&idp.trust_anchor())).unwrap();
    let crl = idp.sign_crl(1, 0, Default::default()).unwrap();
    std::fs::write(dir.join("mesh.crl"), pq_mtls::encode_crl(&crl)).unwrap();
}

fn worker(identities: &Path, service: &str, state: &Path, jobs: &Path, gateway: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_forge-worker"))
        .args([
            "--identity-dir",
            &identities.display().to_string(),
            "--service",
            service,
            "--state-dir",
            &state.display().to_string(),
            "--jobs",
            &jobs.display().to_string(),
            "--gateway",
            gateway,
            "--once",
        ])
        .output()
        .expect("forge-worker runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The `message` of the named job's run line.
fn run_line(stdout: &str, job: &str) -> String {
    stdout
        .lines()
        .find(|l| {
            l.contains("\"message\":\"job run\"") && l.contains(&format!("\"job\":\"{job}\""))
        })
        .unwrap_or_else(|| panic!("no run of {job} in {stdout}"))
        .to_string()
}

fn store(state: &StateDir) -> DurableObjectStore {
    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    DurableObjectStore::open(state.store(), Box::new(sealer), STORE_ID).unwrap()
}

#[test]
fn the_worker_syncs_versioned_datasets_into_the_enclosure_over_the_mesh() {
    let root = scratch("e2e");
    let state = provision(&root.join("enclosure"));
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let identities = root.join("identities");
    issue(&idp, &identities, "http-gateway", &["fabric-gateway"]);
    issue(&idp, &identities, "forge-worker", &["forge-worker"]);
    issue(&idp, &identities, "rogue-worker", &["analyst"]);

    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        data.join("facilities.csv"),
        "key,name\nFAC-N,North\nFAC-S,South\n",
    )
    .unwrap();
    std::fs::write(
        data.join("aircraft.csv"),
        "tail,hours,base\nAC-1,100,FAC-N\nAC-2,,FAC-S\n",
    )
    .unwrap();
    let jobs = data.join("jobs.json");
    std::fs::write(&jobs, JOBS).unwrap();

    let (gateway, _lock) = open_state_dir(&state, Vec::new()).unwrap().into_parts();
    let mesh = spawn_mesh_listener(
        Arc::new(gateway),
        &ServiceIdentity::load(&identities, "http-gateway").unwrap(),
        DefaultPdp::new(policy()),
        "127.0.0.1:0",
        PURPOSE,
    )
    .unwrap()
    .to_string();
    let worker_state = root.join("worker");

    let first = worker(&identities, "forge-worker", &worker_state, &jobs, &mesh);
    assert!(
        first.status.success(),
        "{}{}",
        text(&first.stdout),
        text(&first.stderr)
    );
    let out = text(&first.stdout);
    assert!(
        run_line(&out, "facilities").contains("2 created, 0 updated"),
        "{out}"
    );
    let aircraft = run_line(&out, "aircraft");
    assert!(
        aircraft.contains("2 created, 0 updated, 2 link(s) added"),
        "{aircraft}"
    );
    assert!(aircraft.contains("\"changed\":\"true\""), "{aircraft}");

    let again = worker(&identities, "forge-worker", &worker_state, &jobs, &mesh);
    assert!(again.status.success(), "{}", text(&again.stderr));
    let aircraft = run_line(&text(&again.stdout), "aircraft");
    assert!(aircraft.contains("\"changed\":\"false\""), "{aircraft}");
    assert!(
        aircraft.contains("0 created, 2 updated, 0 link(s) added"),
        "{aircraft}"
    );

    std::fs::write(
        data.join("aircraft.csv"),
        "tail,hours,base\nAC-1,100,FAC-N\nAC-2,250,FAC-S\n",
    )
    .unwrap();
    let changed = worker(&identities, "forge-worker", &worker_state, &jobs, &mesh);
    assert!(changed.status.success(), "{}", text(&changed.stderr));
    assert!(run_line(&text(&changed.stdout), "aircraft").contains("\"changed\":\"true\""));

    let rogue = worker(
        &identities,
        "rogue-worker",
        &root.join("rogue"),
        &jobs,
        &mesh,
    );
    assert_eq!(rogue.status.code(), Some(1));
    assert!(
        text(&rogue.stdout).contains("refused the sync"),
        "{}",
        text(&rogue.stdout)
    );

    let fabric = store(&state);
    assert_eq!(
        fabric
            .get_object("aircraft/AC-2")
            .and_then(|o| o.properties.get("flight_hours")),
        Some(&Value::Int(250))
    );
    assert_eq!(
        fabric
            .get_object("facility/FAC-S")
            .and_then(|o| o.properties.get("name")),
        Some(&Value::Text("South".into()))
    );
    assert_eq!(fabric.neighbors("aircraft/AC-1", "aircraft_base").len(), 1);

    // Every distinct version is kept: facilities once, aircraft before and after.
    let worker_identity = ServiceIdentity::load(&identities, "forge-worker").unwrap();
    let datasets = DurableDatasetStore::open(
        worker_state.join("datasets"),
        worker_identity.data_kms().unwrap(),
    )
    .unwrap();
    assert_eq!(datasets.len(), 3);

    let _ = std::fs::remove_dir_all(&root);
}
