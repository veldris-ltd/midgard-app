//! A gateway served with `--checkpoint-mirror`: every audit ledger checkpoint is also
//! written to a directory off the state directory, and a ledger rolled back together
//! with its checkpoint, deleted with it, or left without a mirrored checkpoint refuses
//! to boot, through the library and through the binary. Without the option nothing
//! changes. [NFR-PQ-3] [AC-M-4] [MG-SEC-05]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use attest::Keystore;
use audit_writer::{checkpoint_path, CheckpointMirror, FileCheckpointMirror, LedgerId};
use http_gateway::boot::{
    init_audit_ledger_mirrored, mirror_audit_ledger, open_state_dir, open_state_dir_mirrored,
    DurableGateway,
};
use http_gateway::cli::{parse, Command as Cli};
use http_gateway::http::Request;
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, AUDIT_KEY_ID, IDP_KEY_ID, LEDGER_ID};
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

/// A state directory and, beside it rather than in it, a mirror directory.
struct Dirs {
    root: PathBuf,
    state: StateDir,
    mirror: PathBuf,
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// What `mg-admin init --checkpoint-mirror` and `user add` produce: keys, the trust
/// anchor, a mirrored ledger, a policy granting `operator` Read, and one operator.
fn provision(tag: &str) -> Dirs {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let root =
        std::env::temp_dir().join(format!("cp-mirror-{tag}-{}-{}", std::process::id(), uniq));
    let mirror = root.join("mirror");
    std::fs::create_dir_all(&mirror).unwrap();
    let state = StateDir::new(root.join("state"));
    std::fs::create_dir_all(state.keys()).unwrap();
    std::fs::create_dir_all(state.trust()).unwrap();
    let idp = Keystore::generate(IDP_KEY_ID, iam::IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut OsRng).unwrap();
    idp.write(&state.idp_keystore()).unwrap();
    audit.write(&state.audit_keystore()).unwrap();
    std::fs::write(state.audit_trust_anchor(), audit.export_verifying_key()).unwrap();
    init_audit_ledger_mirrored(&state, Some(&mirror)).unwrap();

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
    directory
        .enroll(
            "olivia",
            Enrollment {
                user: UserRecord {
                    roles: [Role("operator".into())].into_iter().collect(),
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
            },
        )
        .unwrap();
    Dirs {
        root,
        state,
        mirror,
    }
}

fn pin(state: &StateDir) -> audit_writer::PinnedKey {
    audit_writer::PinnedKey::new(
        Keystore::read(&state.audit_keystore())
            .unwrap()
            .verifying_key,
    )
}

/// The count of the checkpoint the mirror holds for the governance ledger.
fn mirrored(dir: &Path) -> Option<u64> {
    FileCheckpointMirror::new(dir)
        .unwrap()
        .latest(&LedgerId(LEDGER_ID.into()))
        .unwrap()
        .map(|c| c.count)
}

/// Boot with the mirror, log in once (an audited event), and shut down.
fn serve_once(d: &Dirs) -> usize {
    let served = open_state_dir_mirrored(&d.state, Vec::new(), &pin(&d.state), Some(&d.mirror))
        .expect("boots with its mirror");
    login(&served.gateway);
    served.gateway.audit_len()
}

fn login(gw: &DurableGateway) {
    let r = gw.handle(
        &Request {
            method: "POST".into(),
            path: "/api/v1/login".into(),
            query: String::new(),
            body: format!(r#"{{"username":"olivia","password":"{PASSWORD}"}}"#),
            authorization: None,
            origin: None,
        },
        NOW,
    );
    assert_eq!(r.status, 200, "olivia logs in: {}", r.body);
}

fn refused(d: &Dirs) -> String {
    match open_state_dir_mirrored(&d.state, Vec::new(), &pin(&d.state), Some(&d.mirror)) {
        Ok(_) => panic!("this enclosure must not boot"),
        Err(e) => e,
    }
}

fn back_up(state: &StateDir) -> (Vec<u8>, Vec<u8>) {
    (
        std::fs::read(state.ledger()).unwrap(),
        std::fs::read(checkpoint_path(&state.ledger())).unwrap(),
    )
}

fn restore(state: &StateDir, (ledger, checkpoint): &(Vec<u8>, Vec<u8>)) {
    std::fs::write(state.ledger(), ledger).unwrap();
    std::fs::write(checkpoint_path(&state.ledger()), checkpoint).unwrap();
}

#[test]
fn a_ledger_rolled_back_with_its_checkpoint_refuses_to_boot_with_a_mirror() {
    let d = provision("rollback");
    assert_eq!(
        mirrored(&d.mirror),
        Some(0),
        "init mirrors the empty checkpoint"
    );
    serve_once(&d);
    let backup = back_up(&d.state);
    let entries = serve_once(&d);
    assert_eq!(mirrored(&d.mirror), Some(entries as u64), "kept up to date");

    // An older backup of both files is consistent on its own: without the mirror it
    // boots, and the entries since are gone unnoticed.
    restore(&d.state, &backup);
    drop(open_state_dir(&d.state, Vec::new(), &pin(&d.state)).expect("consistent locally"));
    // With the mirror it does not.
    let e = refused(&d);
    assert!(e.contains("rolled back"), "{e}");
    assert!(e.contains("BACKUP_RESTORE"), "{e}");

    // Deleting both files is refused too, and nothing is re-created.
    std::fs::remove_file(d.state.ledger()).unwrap();
    std::fs::remove_file(checkpoint_path(&d.state.ledger())).unwrap();
    let e = refused(&d);
    assert!(e.contains("missing"), "{e}");
    assert!(!d.state.ledger().exists());
}

#[test]
fn a_missing_mirrored_checkpoint_fails_closed_until_it_is_re_seeded() {
    let d = provision("reseed");
    let entries = serve_once(&d);
    let file = FileCheckpointMirror::new(&d.mirror)
        .unwrap()
        .path_for(&LedgerId(LEDGER_ID.into()));
    std::fs::remove_file(&file).unwrap();

    let e = refused(&d);
    assert!(e.contains("no checkpoint"), "{e}");
    assert!(e.contains("mg-admin audit checkpoint"), "{e}");

    // The documented recovery: verify under the pin, then re-seed.
    let vouched = mirror_audit_ledger(&d.state, &pin(&d.state), &d.mirror).unwrap();
    assert_eq!(vouched, entries);
    assert_eq!(mirrored(&d.mirror), Some(entries as u64));
    serve_once(&d);

    // Re-seeding never moves the mirror back over a rolled-back ledger.
    let backup = back_up(&d.state);
    serve_once(&d);
    restore(&d.state, &backup);
    let e = mirror_audit_ledger(&d.state, &pin(&d.state), &d.mirror).unwrap_err();
    assert!(e.contains("rolled back"), "{e}");
}

#[test]
fn a_mirror_must_exist_and_lie_outside_the_state_directory() {
    let d = provision("placement");
    let e = match open_state_dir_mirrored(
        &d.state,
        Vec::new(),
        &pin(&d.state),
        Some(&d.root.join("not-mounted")),
    ) {
        Ok(_) => panic!("a missing mirror directory must not boot"),
        Err(e) => e,
    };
    assert!(e.contains("mounted"), "{e}");

    let inside = d.state.root().join("mirror");
    std::fs::create_dir_all(&inside).unwrap();
    let e = match open_state_dir_mirrored(&d.state, Vec::new(), &pin(&d.state), Some(&inside)) {
        Ok(_) => panic!("a mirror inside the state directory must not boot"),
        Err(e) => e,
    };
    assert!(e.contains("inside the state directory"), "{e}");
}

#[test]
fn the_command_line_passes_the_mirror_to_the_ledger() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let Ok(Cli::Serve(serve)) = parse(&args(&["--state-dir", "/s", "--checkpoint-mirror", "/m"]))
    else {
        panic!("parses");
    };
    assert_eq!(serve.checkpoint_mirror.as_deref(), Some("/m"));
    let Ok(Cli::Serve(serve)) = parse(&args(&["--state-dir", "/s"])) else {
        panic!("parses");
    };
    assert_eq!(serve.checkpoint_mirror, None, "no mirror unless asked for");
    assert!(parse(&args(&[
        "--state-dir",
        "/s",
        "--checkpoint-mirror",
        "/m",
        "--checkpoint-mirror",
        "/n"
    ]))
    .is_err());

    // The binary: a rolled-back ledger is refused at boot, before anything listens.
    let d = provision("binary");
    serve_once(&d);
    let backup = back_up(&d.state);
    serve_once(&d);
    restore(&d.state, &backup);
    let out = Command::new(env!("CARGO_BIN_EXE_http-gateway"))
        .args([
            "--state-dir",
            d.state.root().to_str().unwrap(),
            "--checkpoint-mirror",
            d.mirror.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run http-gateway");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("rolled back"), "stderr: {stderr}");
    assert!(!stderr.contains("listening on"), "stderr: {stderr}");
}
