//! A real enclosure with a schema: an ontology file registered at boot, an object in
//! the durable store typed by it, and a declared action that is human-gated, granted by
//! the policy file, and changes the stored object durably. [DD-1] [NFR-AI-1] [RC-10]
//! [AC-M-4]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use attest::Keystore;
use fabric::{DurableObjectStore, ObjectInstance, Value};
use http_gateway::boot::{open_state_dir, DurableGateway};
use http_gateway::enclosure::level_of;
use http_gateway::http::{Request, Response};
use http_gateway::json::{parse, Json};
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, AUDIT_KEY_ID, IDP_KEY_ID, STORE_ID};
use iam::{Credentials, DurableDirectory, Enrollment, PasswordHasher, Pbkdf2Hasher, UserRecord};
use kms::{Kms, KmsSealer};
use policy::{
    Action, Clearance, Effect, Marking, PolicySet, Purpose, Role, Rule, RuleId, Selector,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

const NOW: u64 = 1_800_000_000;
const PASSWORD: &str = "correct horse battery staple";

const ONTOLOGY: &str = r#"{
  "version": 1,
  "objectTypes": [
    { "id": "aircraft", "title": "Aircraft",
      "properties": [ { "name": "tail", "type": "text", "required": true },
                      { "name": "status", "type": "text" } ] }
  ],
  "actions": [
    { "id": "ground_aircraft", "name": "groundAircraft", "title": "Ground aircraft",
      "objectType": "aircraft", "marking": "internal", "purposes": ["operations"],
      "effect": { "kind": "set-property", "property": "status", "value": "grounded" } }
  ]
}"#;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    std::env::temp_dir().join(format!(
        "ontology-boot-{tag}-{}-{}",
        std::process::id(),
        uniq
    ))
}

/// A state directory whose ontology declares aircraft and a human-gated grounding
/// action, whose policy lets operators read, apply and ground, with one operator
/// enrolled and one aircraft already in the durable store — as an import would leave it.
fn provision(dir: &Path) -> StateDir {
    let state = StateDir::new(dir);
    std::fs::create_dir_all(state.keys()).unwrap();
    std::fs::create_dir_all(state.trust()).unwrap();
    let idp = Keystore::generate(IDP_KEY_ID, iam::IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut OsRng).unwrap();
    idp.write(&state.idp_keystore()).unwrap();
    audit.write(&state.audit_keystore()).unwrap();

    let rule = |id: &str, role: &str, action: &str| Rule {
        id: RuleId(id.into()),
        role: Role(role.into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    };
    let policy = PolicySet::new(vec![
        rule("operator-read", "operator", "Read"),
        rule("operator-apply", "operator", "Apply"),
        rule("operator-ground", "operator", "groundAircraft"),
        rule("approver-read", "approver", "Read"),
        rule("approver-approve", "approver", "Approve"),
    ])
    .unwrap();
    std::fs::write(state.policy(), render_policy(&policy).unwrap()).unwrap();
    std::fs::write(state.ontology(), ONTOLOGY).unwrap();

    let internal = level_of("internal").unwrap();
    let directory_sealer = KmsSealer::new(Kms::new(fips_provider(), idp.trust_root()));
    let mut directory =
        DurableDirectory::open(state.directory(), Box::new(directory_sealer)).unwrap();
    // Olivia operates; Victor approves her human gates.
    for (username, role) in [("olivia", "operator"), ("victor", "approver")] {
        directory
            .enroll(
                username,
                Enrollment {
                    user: UserRecord {
                        roles: [Role(role.into())].into_iter().collect(),
                        clearance: Clearance {
                            level: internal,
                            compartments: Default::default(),
                        },
                        purposes: [Purpose("operations".into())].into_iter().collect(),
                    },
                    credentials: Credentials {
                        password_hash: Pbkdf2Hasher::production().hash(PASSWORD.as_bytes()),
                        ..Credentials::default()
                    },
                },
            )
            .unwrap();
    }

    let store_sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let mut store =
        DurableObjectStore::open(state.store(), Box::new(store_sealer), STORE_ID).unwrap();
    store
        .put_object_durable(ObjectInstance {
            id: "aircraft/AC-1".into(),
            type_id: "aircraft".into(),
            properties: BTreeMap::from([
                ("tail".to_string(), Value::Text("MG-001".into())),
                ("status".to_string(), Value::Text("active".into())),
            ]),
            marking: Marking {
                level: internal,
                compartments: Default::default(),
            },
            allowed_purposes: [Purpose("operations".into())].into_iter().collect(),
        })
        .unwrap();
    state
}

fn call(gw: &DurableGateway, method: &str, path: &str, body: &str, token: &str) -> Response {
    gw.handle(
        &Request {
            method: method.into(),
            path: path.into(),
            query: String::new(),
            body: body.into(),
            authorization: (!token.is_empty()).then(|| format!("Bearer {token}")),
            origin: None,
        },
        NOW,
    )
}

fn login(gw: &DurableGateway) -> String {
    login_as(gw, "olivia")
}

fn login_as(gw: &DurableGateway, username: &str) -> String {
    let body = format!(r#"{{"username":"{username}","password":"{PASSWORD}"}}"#);
    let r = call(gw, "POST", "/api/v1/login", &body, "");
    assert_eq!(r.status, 200, "{}", r.body);
    parse(&r.body)
        .unwrap()
        .str_field("token")
        .expect("token")
        .to_string()
}

/// AC-1's status, read through the governed query path.
fn status_of_ac1(gw: &DurableGateway, token: &str) -> String {
    let r = call(
        gw,
        "POST",
        "/api/v1/query",
        r#"{"objectType":"aircraft","maxResults":50}"#,
        token,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let body = parse(&r.body).unwrap();
    let Some(Json::Arr(results)) = body.get("results") else {
        panic!("no results: {}", r.body);
    };
    results
        .iter()
        .find(|o| o.str_field("id") == Some("aircraft/AC-1"))
        .and_then(|o| o.get("propertyValues"))
        .and_then(|p| p.str_field("status"))
        .unwrap_or_else(|| panic!("AC-1 not visible: {}", r.body))
        .to_string()
}

#[test]
fn a_declared_action_changes_a_stored_object_durably() {
    let dir = scratch("action");
    let state = provision(&dir);
    let ground = |gate: &str| {
        format!(r#"{{"actionType":"Ground aircraft","params":{{"target":"aircraft/AC-1"}}{gate}}}"#)
    };

    {
        let served = open_state_dir(&state, Vec::new()).expect("boots");
        assert_eq!((served.object_types, served.actions), (1, 1));
        let gw = &served.gateway;
        let token = login(gw);

        let types = call(gw, "GET", "/api/v1/object-types", "", &token);
        assert_eq!(types.status, 200, "{}", types.body);
        assert!(
            types.body.contains(r#""apiName":"aircraft""#),
            "{}",
            types.body
        );
        assert_eq!(status_of_ac1(gw, &token), "active");

        let ungated = call(gw, "POST", "/api/v1/actions", &ground(""), &token);
        assert_eq!(ungated.status, 403, "{}", ungated.body);
        assert_eq!(
            status_of_ac1(gw, &token),
            "active",
            "a refused action changes nothing"
        );

        let made_up = call(
            gw,
            "POST",
            "/api/v1/actions",
            &ground(r#","humanGateToken":"HG-1""#),
            &token,
        );
        assert_eq!(made_up.status, 403, "{}", made_up.body);

        let requested = call(
            gw,
            "POST",
            "/api/v1/approvals",
            r#"{"actionType":"Ground aircraft","target":"aircraft/AC-1"}"#,
            &token,
        );
        assert_eq!(requested.status, 200, "{}", requested.body);
        let id = parse(&requested.body)
            .unwrap()
            .str_field("id")
            .expect("approval id")
            .to_string();
        let approved = call(
            gw,
            "POST",
            &format!("/api/v1/approvals/{id}/approve"),
            "",
            &login_as(gw, "victor"),
        );
        assert_eq!(approved.status, 200, "{}", approved.body);

        let gated = call(
            gw,
            "POST",
            "/api/v1/actions",
            &ground(&format!(r#","humanGateToken":"{id}""#)),
            &token,
        );
        assert_eq!(gated.status, 200, "{}", gated.body);
        assert_eq!(status_of_ac1(gw, &token), "grounded");
    } // restart

    let served = open_state_dir(&state, Vec::new()).expect("boots again");
    assert_eq!((served.object_types, served.actions), (1, 1));
    let token = login(&served.gateway);
    assert_eq!(
        status_of_ac1(&served.gateway, &token),
        "grounded",
        "the action's effect survives a restart"
    );
    drop(served);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_invalid_ontology_refuses_to_boot() {
    let dir = scratch("invalid");
    let state = provision(&dir);
    std::fs::write(
        state.ontology(),
        r#"{"version":1,"objectTypes":[{"id":"aircraft","title":"Aircraft","properties":[]}],"actions":[{"id":"a","name":"a","title":"A","objectType":"ship","marking":"internal","purposes":["operations"],"effect":{"kind":"set-property","property":"status","value":"x"}}]}"#,
    )
    .unwrap();
    match open_state_dir(&state, Vec::new()) {
        Ok(_) => panic!("an ontology naming an undeclared type must not boot"),
        Err(e) => assert!(e.contains("ontology file refused"), "{e}"),
    }
    assert!(!state.lock().exists(), "a refused boot leaves no lock");
    let _ = std::fs::remove_dir_all(&dir);
}
