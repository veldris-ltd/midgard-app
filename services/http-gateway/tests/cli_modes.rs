//! The binary's start modes, run as a process: a bare address or nothing at all is a
//! usage error; development mode is loopback only with no override; a state-directory
//! gateway refuses a non-loopback bind unless `--allow-non-loopback` says a TLS
//! terminator fronts it; and `--dev` on loopback serves the seeded login.
//! [RC-24] [NFR-SEC-1] [AC-13.1] [AC-13.2] [VLD-ANB-MIDGARD-0032]
#![cfg(feature = "backend-fips")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

use attest::Keystore;
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, AUDIT_KEY_ID, IDP_KEY_ID};
use http_gateway::{DEMO_PASSWORD, DEMO_USERNAME};
use iam::{Credentials, DurableDirectory, Enrollment, PasswordHasher, Pbkdf2Hasher, UserRecord};
use kms::{Kms, KmsSealer};
use policy::{
    Action, ClassificationLevel, Clearance, Effect, PolicySet, Purpose, Role, Rule, RuleId,
    Selector,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

const BIN: &str = env!("CARGO_BIN_EXE_http-gateway");

/// Run the binary to completion. Every refusal exits before binding, so this never
/// blocks; a run that did start would hang the test, which is itself a failure.
fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run http-gateway")
}

/// Assert `args` is a usage error (exit 2) whose message contains every one of `says`,
/// and that nothing listened.
fn usage_error(args: &[&str], says: &[&str]) {
    let out = run(args);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "{args:?} must exit 2; stderr: {stderr}"
    );
    for want in says {
        assert!(
            stderr.contains(want),
            "{args:?}: the refusal must name {want:?}; stderr: {stderr}"
        );
    }
    assert!(
        !stderr.contains("listening on"),
        "{args:?} must not listen: {stderr}"
    );
}

/// A scratch state directory as `mg-admin init` and `user add` leave it: keys, the
/// audit trust anchor, the ledger, a policy file and one enrolled operator.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn provision(tag: &str) -> Scratch {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let root =
        std::env::temp_dir().join(format!("cli-modes-{tag}-{}-{}", std::process::id(), uniq));
    let state = StateDir::new(&root);
    std::fs::create_dir_all(state.keys()).unwrap();
    std::fs::create_dir_all(state.trust()).unwrap();
    let idp = Keystore::generate(IDP_KEY_ID, iam::IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut OsRng).unwrap();
    idp.write(&state.idp_keystore()).unwrap();
    audit.write(&state.audit_keystore()).unwrap();
    std::fs::write(state.audit_trust_anchor(), audit.export_verifying_key()).unwrap();
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
                    password_hash: Pbkdf2Hasher::production().hash(b"correct horse battery staple"),
                    ..Credentials::default()
                },
            },
        )
        .unwrap();
    Scratch(root)
}

/// Spawn the binary and wait for the address it says it listens on. The child is
/// killed when the returned guard drops.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(args: &[&str]) -> (Running, String, String) {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start http-gateway");
    let mut stderr = BufReader::new(child.stderr.take().expect("stderr"));
    let running = Running(child);
    let mut seen = String::new();
    loop {
        let mut line = String::new();
        if stderr.read_line(&mut line).unwrap_or(0) == 0 {
            panic!("{args:?}: the gateway exited before listening: {seen}");
        }
        seen.push_str(&line);
        if let Some(rest) = line
            .trim()
            .strip_prefix("http-gateway (secured) listening on http://")
        {
            return (running, rest.to_string(), seen);
        }
    }
}

/// One HTTP/1.1 exchange with the edge at `addr`.
fn exchange(addr: &str, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect to the edge");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    resp
}

fn health(addr: &str) -> String {
    exchange(
        addr,
        "GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
}

/// [AC-13.1] Neither a bare address nor no arguments selects development mode.
#[test]
fn a_bare_address_or_no_arguments_is_a_usage_error() {
    usage_error(&["127.0.0.1:0"], &["--dev", "--state-dir"]);
    usage_error(&[], &["--dev", "--state-dir"]);
}

/// [AC-13.2] Development mode refuses another interface outright; a state-directory
/// gateway refuses it unless told a TLS terminator fronts it, and then serves.
#[test]
fn non_loopback_binds_are_refused_unless_allowed() {
    let dir = provision("non-loopback");
    let dir = dir.0.to_str().unwrap();

    usage_error(&["--dev", "0.0.0.0:0"], &["TLS", "--allow-non-loopback"]);
    usage_error(
        &["--dev", "0.0.0.0:0", "--allow-non-loopback"],
        &["TLS", "--allow-non-loopback"],
    );
    usage_error(&["--dev", "--bind", "0.0.0.0:0"], &["--dev"]);
    usage_error(
        &["--state-dir", dir, "--bind", "0.0.0.0:0"],
        &["TLS", "--allow-non-loopback"],
    );

    let (_gateway, addr, seen) = start(&[
        "--state-dir",
        dir,
        "--bind",
        "0.0.0.0:0",
        "--allow-non-loopback",
    ]);
    let port = addr.rsplit_once(':').expect("HOST:PORT").1;
    assert!(
        addr.starts_with("0.0.0.0:"),
        "binds all interfaces: {addr}; {seen}"
    );
    let resp = health(&format!("127.0.0.1:{port}"));
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "got: {resp}");
}

/// [AC-13.2] `--dev` on loopback still serves the seeded demo login, which is the
/// only way to reach it.
#[test]
fn dev_mode_serves_the_seeded_login_on_loopback() {
    let (_gateway, addr, seen) = start(&["--dev", "127.0.0.1:0"]);
    assert!(seen.contains("DEVELOPMENT MODE"), "stderr: {seen}");
    assert!(
        addr.starts_with("127.0.0.1:"),
        "listens on loopback: {addr}"
    );
    let body = format!(r#"{{"username":"{DEMO_USERNAME}","password":"{DEMO_PASSWORD}"}}"#);
    let resp = exchange(
        &addr,
        &format!(
            "POST /api/v1/login HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "got: {resp}");
    assert!(resp.contains("\"token\""), "a token is issued: {resp}");
}
