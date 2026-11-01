//! A gateway served from a state directory — persisted keys, a durable store, ledger
//! and identity directory, and the policy file, with nothing compiled in or seeded.
//! An enrolled principal logs in; the policy file decides what they may do; with no
//! ontology loaded a query is refused rather than answered from invented data; a
//! second gateway cannot take a directory already being served; a token and the
//! ledger both survive a restart; and a tampered ledger refuses to boot.
//! [RC-09] [RC-10] [RC-29] [AC-M-4]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use attest::Keystore;
use http_gateway::boot::{open_state_dir, DurableGateway};
use http_gateway::http::{Request, Response};
use http_gateway::json::parse;
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, AUDIT_KEY_ID, IDP_KEY_ID};
use iam::{Credentials, DurableDirectory, Enrollment, PasswordHasher, Pbkdf2Hasher, UserRecord};
use kms::{Kms, KmsSealer};
use policy::{
    Action, ClassificationLevel, Clearance, Effect, PolicySet, Purpose, Role, Rule, RuleId,
    Selector,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

const NOW: u64 = 1_800_000_000;
const PASSWORD: &str = "correct horse battery staple";

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    std::env::temp_dir().join(format!("state-boot-{tag}-{}-{}", std::process::id(), uniq))
}

fn principal(role: &str) -> Enrollment {
    Enrollment {
        user: UserRecord {
            roles: [Role(role.into())].into_iter().collect(),
            clearance: Clearance {
                level: ClassificationLevel::CONFIDENTIAL,
                compartments: Default::default(),
            },
            purposes: [Purpose("operations".into())].into_iter().collect(),
        },
        credentials: Credentials {
            password_hash: Pbkdf2Hasher::production().hash(PASSWORD.as_bytes()),
            ..Credentials::default()
        },
    }
}

/// What `mg-admin init` and `user add` produce: keys, a policy file granting the
/// `operator` role Read, and two enrolled principals — an operator and a viewer the
/// policy grants nothing.
fn provision(dir: &Path) -> StateDir {
    let state = StateDir::new(dir);
    std::fs::create_dir_all(state.keys()).unwrap();
    std::fs::create_dir_all(state.trust()).unwrap();
    let idp = Keystore::generate(IDP_KEY_ID, iam::IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut OsRng).unwrap();
    idp.write(&state.idp_keystore()).unwrap();
    audit.write(&state.audit_keystore()).unwrap();
    // What `mg-admin init` also creates: the ledger and its signed checkpoint.
    http_gateway::boot::init_audit_ledger(&state).unwrap();

    let policy = PolicySet::new(vec![Rule {
        id: RuleId("operator-read".into()),
        role: Role("operator".into()),
        action: Action("Read".into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    }])
    .unwrap();
    std::fs::write(state.policy(), render_policy(&policy).unwrap()).unwrap();

    let sealer = KmsSealer::new(Kms::new(fips_provider(), idp.trust_root()));
    let mut directory = DurableDirectory::open(state.directory(), Box::new(sealer)).unwrap();
    directory.enroll("olivia", principal("operator")).unwrap();
    directory.enroll("victor", principal("viewer")).unwrap();
    state
}

fn call(
    gw: &DurableGateway,
    method: &str,
    path: &str,
    body: &str,
    token: Option<&str>,
) -> Response {
    gw.handle(
        &Request {
            method: method.into(),
            path: path.into(),
            query: String::new(),
            body: body.into(),
            authorization: token.map(|t| format!("Bearer {t}")),
            origin: None,
        },
        NOW,
    )
}

fn login(gw: &DurableGateway, username: &str) -> String {
    let body = format!(r#"{{"username":"{username}","password":"{PASSWORD}"}}"#);
    let r = call(gw, "POST", "/api/v1/login", &body, None);
    assert_eq!(r.status, 200, "{username} logs in: {}", r.body);
    parse(&r.body)
        .unwrap()
        .str_field("token")
        .expect("a token")
        .to_string()
}

#[test]
fn a_state_directory_serves_a_real_enclosure_across_restarts() {
    let dir = scratch("serve");
    let state = provision(&dir);

    let (olivia_token, audited) = {
        let served =
            open_state_dir(&state, Vec::new(), &pin(&state)).expect("the state directory boots");
        assert_eq!(served.principals, 2);
        assert_eq!(served.audit_entries, 0, "a fresh ledger");
        let gw = &served.gateway;

        // Nothing compiled in: the demo credential does not exist here.
        let demo = call(
            gw,
            "POST",
            "/api/v1/login",
            r#"{"username":"operator","password":"midgard-demo"}"#,
            None,
        );
        assert_eq!(demo.status, 401);

        // The policy file decides: the operator may read, the viewer may not.
        let olivia = login(gw, "olivia");
        let types = call(gw, "GET", "/api/v1/object-types", "", Some(&olivia));
        assert_eq!(types.status, 200, "{}", types.body);
        let victor = login(gw, "victor");
        assert_eq!(
            call(gw, "GET", "/api/v1/object-types", "", Some(&victor)).status,
            403,
            "no rule grants the viewer role anything"
        );

        // Nothing seeded: no ontology is loaded, so a query is refused, not invented.
        let query = call(
            gw,
            "POST",
            "/api/v1/query",
            r#"{"objectType":"aircraft"}"#,
            Some(&olivia),
        );
        assert_eq!(query.status, 404, "{}", query.body);

        // Only one process serves a state directory.
        let second = open_state_dir(&state, Vec::new(), &pin(&state));
        assert!(
            matches!(&second, Err(e) if e.contains("in use")),
            "a second gateway is refused"
        );

        (olivia, gw.audit_len())
    }; // gateway and lock dropped: a restart

    let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("it boots again");
    assert_eq!(
        served.audit_entries, audited,
        "every decision from the first run is recovered and re-verified"
    );
    let reused = call(
        &served.gateway,
        "GET",
        "/api/v1/object-types",
        "",
        Some(&olivia_token),
    );
    assert_eq!(
        reused.status, 200,
        "a token from before the restart still verifies: {}",
        reused.body
    );
    drop(served);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tampered_ledger_refuses_to_boot() {
    let dir = scratch("tamper");
    let state = provision(&dir);
    {
        let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
        // The login is recorded in the ledger, and so is the authorised request.
        let token = login(&served.gateway, "olivia");
        assert_eq!(served.gateway.audit_len(), 1, "the login is audited");
        let r = call(
            &served.gateway,
            "GET",
            "/api/v1/object-types",
            "",
            Some(&token),
        );
        assert_eq!(r.status, 200, "{}", r.body);
        assert!(served.gateway.audit_len() > 0);
    }

    let mut bytes = std::fs::read(state.ledger()).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    std::fs::write(state.ledger(), bytes).unwrap();

    match open_state_dir(&state, Vec::new(), &pin(&state)) {
        Ok(_) => panic!("a tampered ledger must not boot"),
        Err(e) => assert!(e.contains("audit ledger"), "{e}"),
    }
    assert!(
        !state.lock().exists(),
        "a refused boot does not leave the directory locked"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Drop the last length-framed entry from the ledger file.
fn cut_last_entry(ledger: &Path) {
    let buf = std::fs::read(ledger).unwrap();
    let mut pos = 0;
    let mut last = 0;
    while pos < buf.len() {
        last = pos;
        pos +=
            4 + u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]) as usize;
    }
    std::fs::write(ledger, &buf[..last]).unwrap();
}

fn refused(state: &StateDir, pin: &audit_writer::PinnedKey) -> String {
    match open_state_dir(state, Vec::new(), pin) {
        Ok(_) => panic!("this enclosure must not boot"),
        Err(e) => e,
    }
}

#[test]
fn a_shortened_or_deleted_ledger_refuses_to_boot() {
    let dir = scratch("shortened");
    let state = provision(&dir);
    {
        let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
        login(&served.gateway, "olivia");
        login(&served.gateway, "victor");
        assert!(served.gateway.audit_len() >= 2);
    }

    // Cutting the last entry off leaves a valid but shorter chain; the signed
    // checkpoint still counts it. [MG-SEC-05]
    cut_last_entry(&state.ledger());
    let e = refused(&state, &pin(&state));
    assert!(e.contains("truncated"), "{e}");

    // Deleting the ledger is not a fresh start.
    std::fs::remove_file(state.ledger()).unwrap();
    let e = refused(&state, &pin(&state));
    assert!(e.contains("missing"), "{e}");
    // Nor is deleting its checkpoint along with it.
    std::fs::remove_file(audit_writer::checkpoint_path(&state.ledger())).unwrap();
    let e = refused(&state, &pin(&state));
    assert!(e.contains("missing"), "{e}");
    assert!(
        !state.ledger().exists(),
        "a refused boot does not re-create it"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_audit_keystore_that_is_not_the_pinned_key_refuses_to_boot() {
    let dir = scratch("unpinned");
    let state = provision(&dir);
    let genuine = pin(&state);
    drop(open_state_dir(&state, Vec::new(), &genuine).expect("boots under its pin"));

    // Checked against another key, the same enclosure does not boot.
    let other = Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut OsRng).unwrap();
    let wrong = audit_writer::PinnedKey::new(other.verifying_key.clone());
    let e = refused(&state, &wrong);
    assert!(e.contains("pinned audit key"), "{e}");

    // A swapped-in audit keystore (with a ledger of its own making) does not get to
    // vouch for itself: the gateway still checks it against the operator's pin.
    other.write(&state.audit_keystore()).unwrap();
    std::fs::remove_file(state.ledger()).unwrap();
    std::fs::remove_file(audit_writer::checkpoint_path(&state.ledger())).unwrap();
    http_gateway::boot::init_audit_ledger(&state).unwrap();
    let e = refused(&state, &genuine);
    assert!(e.contains("pinned audit key"), "{e}");

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
