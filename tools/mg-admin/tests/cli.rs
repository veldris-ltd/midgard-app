//! `mg-admin` end to end, driving the real binary: `init` lays out a state directory
//! the gateway can serve, `user add` enrols principals from a piped password and
//! refuses everything it should, `user list` shows them, and a principal it enrolled
//! logs in through a real gateway over the same directory. [RC-29] [NFR-SEC-2]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use attest::Keystore;
use audit_writer::{verify_chain, FileLedger, LedgerId, LedgerStore};
use governance::Governance;
use http_gateway::http::Request;
use http_gateway::ontology_file::parse_ontology;
use http_gateway::policy_file::parse_policy;
use http_gateway::state_dir::{StateDir, LEDGER_ID};
use http_gateway::SecuredGateway;
use iam::{DurableDirectory, Idp, Pbkdf2Hasher};
use kms::{Kms, KmsSealer};
use policy::DefaultPdp;
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{OsRng, SigId, VerifyingKey};

const PASSWORD: &str = "correct horse battery staple";

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    std::env::temp_dir().join(format!("mg-admin-{tag}-{}-{}", std::process::id(), uniq))
}

/// Run `mg-admin` with `args`, feeding `stdin` through a pipe.
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

fn init(dir: &Path) {
    let o = mg_admin(&["init", "--state-dir", &dir.display().to_string()], "");
    assert!(o.status.success(), "init: {}", stderr(&o));
}

fn add_user(dir: &Path, subject: &str, extra: &[&str], password: &str) -> Output {
    let dir = dir.display().to_string();
    let mut args = vec![
        "user",
        "add",
        "--state-dir",
        &dir,
        "--subject",
        subject,
        "--roles",
        "operator",
        "--clearance",
        "internal",
        "--purposes",
        "operations",
    ];
    args.extend_from_slice(extra);
    mg_admin(&args, &format!("{password}\n"))
}

#[test]
fn init_lays_out_a_state_directory_and_never_overwrites_one() {
    let dir = scratch("init");
    init(&dir);
    let state = StateDir::new(&dir);

    for path in [
        state.idp_keystore(),
        state.audit_keystore(),
        state.idp_trust_anchor(),
        state.audit_trust_anchor(),
        state.policy(),
        state.ontology(),
    ] {
        assert!(path.is_file(), "{} exists", path.display());
    }
    for (anchor, key_id) in [
        (state.idp_trust_anchor(), "midgard-idp"),
        (state.audit_trust_anchor(), "midgard-audit"),
    ] {
        let line = std::fs::read_to_string(&anchor).unwrap();
        assert!(
            line.starts_with(&format!("ML-DSA-65 {key_id} ")),
            "{}: {line}",
            anchor.display()
        );
    }
    let policy = parse_policy(&std::fs::read_to_string(state.policy()).unwrap())
        .expect("the starter policy loads");
    assert_eq!(policy.rules().len(), 4);
    let ontology = parse_ontology(&std::fs::read_to_string(state.ontology()).unwrap())
        .expect("the starter ontology loads");
    assert!(ontology.object_types.is_empty());

    let again = mg_admin(&["init", "--state-dir", &dir.display().to_string()], "");
    assert_eq!(again.status.code(), Some(1), "{}", stderr(&again));
    assert!(stderr(&again).contains("not empty"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn user_add_enrols_and_refuses_what_it_should() {
    let dir = scratch("users");
    init(&dir);
    let dir_arg = dir.display().to_string();

    let plain = add_user(&dir, "alice", &[], PASSWORD);
    assert!(plain.status.success(), "{}", stderr(&plain));
    assert!(stdout(&plain).contains("enrolled 'alice'"));
    assert!(!stdout(&plain).contains("otpauth://"));

    let with_totp = add_user(&dir, "bob", &["--totp"], PASSWORD);
    assert!(with_totp.status.success(), "{}", stderr(&with_totp));
    assert!(stdout(&with_totp).contains("otpauth://totp/Midgard:bob?secret="));

    let duplicate = add_user(&dir, "alice", &[], PASSWORD);
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(stderr(&duplicate).contains("already enrolled"));

    let short = add_user(&dir, "carol", &[], "short");
    assert_eq!(short.status.code(), Some(2));

    let bad_band = mg_admin(
        &[
            "user",
            "add",
            "--state-dir",
            &dir_arg,
            "--subject",
            "dave",
            "--roles",
            "operator",
            "--clearance",
            "top-secret",
            "--purposes",
            "operations",
        ],
        &format!("{PASSWORD}\n"),
    );
    assert_eq!(bad_band.status.code(), Some(2));

    let typo = mg_admin(
        &["user", "add", "--state-dir", &dir_arg, "--subjet", "erin"],
        &format!("{PASSWORD}\n"),
    );
    assert_eq!(typo.status.code(), Some(2));
    assert!(stderr(&typo).contains("unknown option"));

    let listed = mg_admin(&["user", "list", "--state-dir", &dir_arg], "");
    assert!(listed.status.success(), "{}", stderr(&listed));
    let text = stdout(&listed);
    assert!(text.starts_with("2 principal(s)"), "{text}");
    assert!(text.contains(
        "alice  roles=operator  clearance=internal  purposes=operations  second-factor=no"
    ));
    assert!(text.contains(
        "bob  roles=operator  clearance=internal  purposes=operations  second-factor=yes"
    ));

    // Exactly the two enrolments are in the governance ledger — none of the refused
    // attempts — and the chain verifies under the audit key.
    let state = StateDir::new(&dir);
    let audit = Keystore::read(&state.audit_keystore()).expect("audit keystore");
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let ledger = FileLedger::open(state.ledger(), Box::new(sealer), LedgerId(LEDGER_ID.into()))
        .expect("the ledger opens");
    let rows: Vec<(&str, &str, &str)> = ledger
        .entries()
        .iter()
        .map(|e| {
            (
                e.record.actor.0.as_str(),
                e.record.action.0.as_str(),
                e.record.resource.0.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("mg-admin", "ENROLL principal", "principal:alice"),
            ("mg-admin", "ENROLL principal", "principal:bob"),
        ]
    );
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
    .expect("the enrolment chain verifies");
    drop(ledger);

    // A held lock — a running gateway — refuses the write.
    let state = StateDir::new(&dir);
    std::fs::write(state.lock(), "http-gateway (pid 1)\n").unwrap();
    let locked = add_user(&dir, "frank", &[], PASSWORD);
    assert_eq!(locked.status.code(), Some(1));
    assert!(stderr(&locked).contains("in use"), "{}", stderr(&locked));
    std::fs::remove_file(state.lock()).unwrap();

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_principal_enrolled_by_mg_admin_logs_in_through_the_gateway() {
    let dir = scratch("login");
    init(&dir);
    let added = add_user(&dir, "alice", &[], PASSWORD);
    assert!(added.status.success(), "{}", stderr(&added));

    // Open the directory the way a gateway serving this state directory does.
    let state = StateDir::new(&dir);
    let keystore = Keystore::read(&state.idp_keystore()).expect("issuer keystore");
    let sealer = KmsSealer::new(Kms::new(fips_provider(), keystore.trust_root()));
    let directory =
        DurableDirectory::open(state.directory(), Box::new(sealer)).expect("directory opens");

    let idp = Idp::generate("test-idp", &mut OsRng).expect("idp");
    let issuer_vk = idp.verifying_key().clone();
    let governance = Governance::new(
        DefaultPdp::new(http_gateway::dev_login_policy()),
        &mut OsRng,
    )
    .expect("governance");
    let gateway = SecuredGateway::new(fips_provider(), issuer_vk, governance, Vec::new())
        .with_idp(idp, Pbkdf2Hasher::production())
        .with_directory(directory);

    let login = |password: &str| {
        gateway
            .handle(
                &Request {
                    method: "POST".into(),
                    path: "/api/v1/login".into(),
                    query: String::new(),
                    body: format!(r#"{{"username":"alice","password":"{password}"}}"#),
                    authorization: None,
                    origin: None,
                },
                1_800_000_000,
            )
            .status
    };
    assert_eq!(login(PASSWORD), 200);
    assert_eq!(login("not the password"), 401);

    let _ = std::fs::remove_dir_all(&dir);
}
