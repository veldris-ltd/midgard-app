//! `mg-admin import` end to end, driving the real binary: objects and links from a JSON
//! Lines file land in a real enclosure's durable store, checked against its ontology and
//! recorded in its audit ledger, never replacing what is there. A gateway serving the
//! directory then answers queries with them, under each object's marking.
//! [DD-1] [RC-10] [AC-M-4]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use attest::Keystore;
use audit_writer::{verify_chain, FileLedger, LedgerId, LedgerStore};
use http_gateway::boot::open_state_dir;
use http_gateway::http::Request;
use http_gateway::json::{parse, Json};
use http_gateway::state_dir::{StateDir, LEDGER_ID};
use kms::{Kms, KmsSealer};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{SigId, VerifyingKey};

const PASSWORD: &str = "correct horse battery staple";

const ONTOLOGY: &str = r#"{
  "version": 1,
  "objectTypes": [
    { "id": "aircraft", "title": "Aircraft",
      "properties": [ { "name": "tail", "type": "text", "required": true },
                      { "name": "status", "type": "text" } ] },
    { "id": "facility", "title": "Facility",
      "properties": [ { "name": "name", "type": "text", "required": true } ] }
  ],
  "linkTypes": [
    { "id": "aircraft_base", "name": "based at", "from": "aircraft", "to": "facility",
      "cardinality": "one-to-one" }
  ]
}"#;

/// Two aircraft — one in a compartment the operator will not hold — a facility, and a
/// link.
const DATA: &str = r#"{"object":"aircraft","key":"AC-1","marking":"internal","purposes":["operations"],"properties":{"tail":"MG-001","status":"active"}}
{"object":"aircraft","key":"AC-2","marking":"internal","compartments":["SIG"],"purposes":["operations"],"properties":{"tail":"MG-002","status":"active"}}
{"object":"facility","key":"FAC-N","marking":"internal","purposes":["operations"],"properties":{"name":"North"}}
{"link":"aircraft_base","from":"AC-1","to":"FAC-N"}
"#;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    std::env::temp_dir().join(format!(
        "mg-admin-import-{tag}-{}-{}",
        std::process::id(),
        uniq
    ))
}

fn mg_admin(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mg-admin"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mg-admin starts");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(stdin.as_bytes())
        .expect("stdin accepts input");
    child.wait_with_output().expect("mg-admin exits")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn import(dir: &Path, file: &Path) -> Output {
    mg_admin(
        &[
            "import",
            "--state-dir",
            &dir.display().to_string(),
            "--file",
            &file.display().to_string(),
        ],
        "",
    )
}

#[test]
fn an_import_fills_a_real_enclosure_that_a_gateway_then_serves() {
    let dir = scratch("serve");
    let dir_arg = dir.display().to_string();
    let init = mg_admin(&["init", "--state-dir", &dir_arg], "");
    assert!(init.status.success(), "{}", stderr(&init));
    let state = StateDir::new(&dir);
    std::fs::write(state.ontology(), ONTOLOGY).unwrap();

    let data = dir.with_extension("data.jsonl");
    std::fs::write(&data, DATA).unwrap();
    let first = import(&dir, &data);
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(
        stdout(&first).contains("imported 3 object(s) and 1 link(s)"),
        "{}",
        stdout(&first)
    );

    // Importing the same file again would replace objects: refused, nothing recorded.
    let again = import(&dir, &data);
    assert_eq!(again.status.code(), Some(1));
    assert!(
        stderr(&again).contains("already in the enclosure"),
        "{}",
        stderr(&again)
    );

    // A file with a bad line is refused before anything is written or recorded.
    let bad = dir.with_extension("bad.jsonl");
    std::fs::write(
        &bad,
        r#"{"object":"aircraft","key":"AC-3","marking":"internal","purposes":["operations"],"properties":{"tail":"MG-003","colour":"red"}}"#,
    )
    .unwrap();
    let refused = import(&dir, &bad);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).contains("import refused: line 1"),
        "{}",
        stderr(&refused)
    );

    let enrol = mg_admin(
        &[
            "user",
            "add",
            "--state-dir",
            &dir_arg,
            "--subject",
            "olivia",
            "--roles",
            "operator",
            "--clearance",
            "internal",
            "--purposes",
            "operations",
        ],
        &format!("{PASSWORD}\n"),
    );
    assert!(enrol.status.success(), "{}", stderr(&enrol));

    // The ledger records the one import that happened and the enrolment, and verifies.
    {
        let audit = Keystore::read(&state.audit_keystore()).expect("audit keystore");
        let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
        let ledger = FileLedger::open(state.ledger(), Box::new(sealer), LedgerId(LEDGER_ID.into()))
            .expect("the ledger opens");
        let actions: Vec<&str> = ledger
            .entries()
            .iter()
            .map(|e| e.record.action.0.as_str())
            .collect();
        assert_eq!(actions, ["IMPORT data", "ENROLL principal"]);
        let audit_vk = audit.verifying_key.clone();
        let vk_for = move |alg: SigId| -> Option<VerifyingKey> {
            (alg == governance::GOV_SIG_ALG).then(|| audit_vk.clone())
        };
        verify_chain(
            ledger.entries(),
            &fips_provider(),
            &LedgerId(LEDGER_ID.into()),
            &vk_for,
        )
        .expect("the chain verifies");
    }

    // A gateway over the directory serves the imported objects, under their markings.
    let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("the enclosure boots");
    let gw = &served.gateway;
    let request = |method: &str, path: &str, body: &str, token: Option<&str>| {
        gw.handle(
            &Request {
                method: method.into(),
                path: path.into(),
                query: String::new(),
                body: body.into(),
                authorization: token.map(|t| format!("Bearer {t}")),
                origin: None,
            },
            1_800_000_000,
        )
    };
    let login = request(
        "POST",
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

    let query = request(
        "POST",
        "/api/v1/query",
        r#"{"objectType":"aircraft","maxResults":50}"#,
        Some(&token),
    );
    assert_eq!(query.status, 200, "{}", query.body);
    let body = parse(&query.body).unwrap();
    let Some(Json::Arr(results)) = body.get("results") else {
        panic!("no results: {}", query.body);
    };
    let ids: Vec<&str> = results.iter().filter_map(|o| o.str_field("id")).collect();
    assert_eq!(
        ids,
        ["aircraft/AC-1"],
        "the SIG-compartmented aircraft is hidden from a caller without SIG"
    );
    drop(served);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&data);
    let _ = std::fs::remove_file(&bad);
}

/// The audit key `mg-admin init` exported, pinned as an operator would pin it.
/// [MG-SEC-05]
fn pin(state: &StateDir) -> audit_writer::PinnedKey {
    http_gateway::boot::read_audit_pin(&state.audit_trust_anchor()).unwrap()
}
