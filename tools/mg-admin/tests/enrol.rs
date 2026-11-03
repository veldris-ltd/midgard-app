//! `mg-admin admin add` and `mg-admin enrol` end to end: the real binary against a real
//! gateway on a socket. Two IAM administrators get passkeys; one requests an enrolment,
//! cannot approve it herself, and the other approves it, receiving a single-use
//! activation code; the principal activates with it, takes their TOTP factor and logs
//! in with both factors. A waiting enrolment can be rejected. A gateway that is not on
//! loopback is refused. [FR-AD-5] [RC-29] [RC-30] [AC-15.3] [AC-15.5]
//! [VLD-ANB-MIDGARD-0002] [VLD-ANB-MIDGARD-0066]
//!
//! Every enrol command also sends a one-time code from the TOTP secret `admin add`
//! printed, and the gateway accepts each code once, so the tests hand each command a
//! code for a step it has not used. [AC-10.4] [VLD-ANB-MIDGARD-0008]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use http_gateway::boot::open_state_dir;
use http_gateway::state_dir::StateDir;

const PASSWORD: &str = "correct horse battery staple";

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    std::env::temp_dir().join(format!(
        "mg-admin-enrol-{tag}-{}-{}",
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

/// Serve the state directory on a free loopback port for the rest of the test process.
fn serve(dir: &Path) -> String {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let served = open_state_dir(&StateDir::new(dir), Vec::new(), &pin(&StateDir::new(dir)))
        .expect("the enclosure boots");
    let addr = format!("127.0.0.1:{port}");
    let bind = addr.clone();
    thread::spawn(move || {
        let (gateway, _lock) = served.into_parts();
        http_gateway::serve_gateway(&bind, gateway).expect("serves");
    });
    for _ in 0..100 {
        if TcpStream::connect(&addr).is_ok() {
            return format!("http://{addr}");
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("the gateway never listened on {addr}");
}

/// POST `body` to `path` on the gateway: the status and the response body.
fn post(gateway: &str, path: &str, body: &str) -> (u16, String) {
    let addr = gateway.trim_start_matches("http://");
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let status = reply
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status");
    let body = reply
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    (status, body)
}

fn login_status(gateway: &str, totp: Option<&str>) -> u16 {
    let totp = totp.map_or(String::new(), |code| format!(r#","totp":"{code}""#));
    let body = format!(r#"{{"username":"olivia","password":"{PASSWORD}"{totp}}}"#);
    post(gateway, "/api/v1/login", &body).0
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// The TOTP secret in an `otpauth://` URI (RFC 4648 base32, unpadded).
fn totp_of(uri: &str) -> iam::TotpSecret {
    let encoded = uri
        .split(['?', '&', '"'])
        .find_map(|part| part.strip_prefix("secret="))
        .expect("a secret parameter");
    let (mut bits, mut acc, mut out) = (0u32, 0u32, Vec::new());
    for c in encoded.chars() {
        let v = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
            .find(c.to_ascii_uppercase())
            .expect("base32") as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    iam::TotpSecret::new(out)
}

/// Administrators' TOTP secrets, as `admin add` printed them, and the last step each
/// has sent a code for.
#[derive(Default)]
struct Codes {
    secrets: RefCell<BTreeMap<String, iam::TotpSecret>>,
    used: RefCell<BTreeMap<String, u64>>,
}

impl Codes {
    /// Keep the secret in the `otpauth://` URI `printed` shows for `admin`.
    fn learn(&self, admin: &str, printed: &str) {
        assert!(
            printed.contains(&format!("otpauth://totp/Midgard:{admin}?")),
            "{printed}"
        );
        self.secrets
            .borrow_mut()
            .insert(admin.to_string(), totp_of(printed));
    }

    /// A code for `admin` the gateway has not seen: the current step's, else the next
    /// one's (accepted within the gateway's one step of skew), else, once both are
    /// spent, the current one's after waiting for the clock.
    fn code(&self, admin: &str) -> String {
        let secrets = self.secrets.borrow();
        let secret = secrets.get(admin).expect("a known administrator");
        loop {
            let current = now() / 30;
            let step = self
                .used
                .borrow()
                .get(admin)
                .map_or(current, |last| current.max(last + 1));
            if step <= current + 1 {
                self.used.borrow_mut().insert(admin.to_string(), step);
                return secret.code_at(step * 30);
            }
            thread::sleep(Duration::from_millis(500));
        }
    }
}

/// `mg-admin init` a state directory with IAM administrators alice and bob: the
/// directory, where each administrator's passkey is, and their TOTP secrets.
fn two_administrators(tag: &str) -> (PathBuf, impl Fn(&str) -> PathBuf, Codes) {
    let dir = scratch(tag);
    let dir_arg = dir.display().to_string();
    let init = mg_admin(&["init", "--state-dir", &dir_arg], "");
    assert!(init.status.success(), "{}", stderr(&init));

    let base = dir.clone();
    let keystore = move |admin: &str| base.with_extension(format!("{admin}.passkey"));
    let codes = Codes::default();
    for admin in ["alice", "bob"] {
        let path = keystore(admin).display().to_string();
        let added = mg_admin(
            &[
                "admin",
                "add",
                "--state-dir",
                &dir_arg,
                "--admin",
                admin,
                "--role",
                "IAM-Admin",
                "--keystore",
                &path,
            ],
            "",
        );
        assert!(added.status.success(), "{}", stderr(&added));
        assert!(keystore(admin).is_file());
        codes.learn(admin, &stdout(&added));
    }
    (dir, keystore, codes)
}

/// The arguments `enrol request` takes for olivia.
const REQUEST_OLIVIA: &[&str] = &[
    "--subject",
    "olivia",
    "--roles",
    "operator",
    "--clearance",
    "internal",
    "--purposes",
    "operations",
    "--justification",
    "new operator",
];

/// The enrolment id `enrol request` printed.
fn enrolment_id(requested: &Output) -> String {
    let text = stdout(requested);
    text.split_whitespace()
        .find(|w| w.starts_with("enrol-olivia-"))
        .unwrap_or_else(|| panic!("an enrolment id in: {text}"))
        .to_string()
}

#[test]
fn two_administrators_enrol_a_principal_through_a_running_gateway() {
    let (dir, keystore, codes) = two_administrators("dual");
    let dir_arg = dir.display().to_string();
    let again = mg_admin(
        &[
            "admin",
            "add",
            "--state-dir",
            &dir_arg,
            "--admin",
            "carol",
            "--role",
            "IAM-Admin",
            "--keystore",
            &keystore("alice").display().to_string(),
        ],
        "",
    );
    assert_eq!(
        again.status.code(),
        Some(1),
        "an existing passkey file is never overwritten"
    );

    let gateway = serve(&dir);
    let as_admin = |admin: &str| {
        vec![
            "--gateway".to_string(),
            gateway.clone(),
            "--admin".to_string(),
            admin.to_string(),
            "--keystore".to_string(),
            keystore(admin).display().to_string(),
        ]
    };
    let run = |command: &[&str], admin: &str, extra: &[&str]| {
        let mut args: Vec<String> = command.iter().map(|s| s.to_string()).collect();
        args.extend(as_admin(admin));
        args.extend(["--totp".to_string(), codes.code(admin)]);
        args.extend(extra.iter().map(|s| s.to_string()));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        mg_admin(&refs, "")
    };

    // No password is read and none is sent: the request names who and what only.
    let requested = run(&["enrol", "request"], "alice", REQUEST_OLIVIA);
    assert!(requested.status.success(), "{}", stderr(&requested));
    let id = enrolment_id(&requested);
    assert_eq!(
        login_status(&gateway, None),
        401,
        "not enrolled until approved"
    );

    let own = run(
        &["enrol", "approve"],
        "alice",
        &["--id", &id, "--subject", "olivia"],
    );
    assert_eq!(own.status.code(), Some(1));
    // The gateway says only `refused`; the privilege service's reason
    // (SelfApprovalDenied) stays in its log. [VLD-ANB-MIDGARD-0053]
    assert!(
        stderr(&own).contains("the gateway refused (403): refused"),
        "{}",
        stderr(&own)
    );

    let listed = run(&["enrol", "list"], "bob", &[]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    let text = stdout(&listed);
    assert!(
        text.contains(&id)
            && text.contains("requested-by=alice")
            && text.contains("roles=operator")
            && text.contains("purposes=operations")
            && text.contains("mfa=totp at activation"),
        "{text}"
    );

    let approved = run(
        &["enrol", "approve"],
        "bob",
        &["--id", &id, "--subject", "olivia"],
    );
    assert!(approved.status.success(), "{}", stderr(&approved));
    let text = stdout(&approved);
    assert!(text.contains("'olivia' is enrolled"), "{text}");
    assert!(
        !text.contains("otpauth"),
        "the approver never sees the second factor"
    );
    let code = text
        .split_whitespace()
        .find(|w| w.len() == 32 && w.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or_else(|| panic!("an activation code in: {text}"))
        .to_string();
    assert!(stdout(&run(&["enrol", "list"], "bob", &[])).starts_with("0 enrolment(s)"));
    assert_eq!(
        login_status(&gateway, None),
        401,
        "not until olivia activates"
    );

    // Olivia activates with the code bob handed her, and takes her second factor.
    let (status, body) = post(
        &gateway,
        "/api/v1/activate",
        &format!(r#"{{"username":"olivia","activationCode":"{code}","password":"{PASSWORD}"}}"#),
    );
    assert_eq!(status, 200, "{body}");
    let totp = totp_of(&body);
    assert_eq!(
        login_status(&gateway, None),
        401,
        "the password alone is not enough"
    );
    assert_eq!(
        login_status(&gateway, Some(&totp.code_at(now()))),
        200,
        "olivia logs in with both factors once activated"
    );

    // Administration never leaves loopback.
    let mut remote: Vec<String> = ["enrol", "list"].iter().map(|s| s.to_string()).collect();
    remote.extend(as_admin("bob"));
    remote.extend(["--totp".to_string(), "000000".to_string()]);
    remote[3] = "http://192.0.2.10:7710".to_string();
    let refs: Vec<&str> = remote.iter().map(String::as_str).collect();
    let refused = mg_admin(&refs, "");
    assert_eq!(refused.status.code(), Some(2), "{}", stderr(&refused));
    assert!(
        stderr(&refused).contains("loopback"),
        "{}",
        stderr(&refused)
    );

    let _ = std::fs::remove_dir_all(&dir);
    for admin in ["alice", "bob"] {
        let _ = std::fs::remove_file(keystore(admin));
    }
}

#[test]
fn enrol_reject_removes_a_parked_enrolment() {
    let (dir, keystore, codes) = two_administrators("reject");
    let gateway = serve(&dir);
    let run = |command: &[&str], admin: &str, extra: &[&str]| {
        let mut args: Vec<String> = command.iter().map(|s| s.to_string()).collect();
        args.extend([
            "--gateway".to_string(),
            gateway.clone(),
            "--admin".to_string(),
            admin.to_string(),
            "--keystore".to_string(),
            keystore(admin).display().to_string(),
            "--totp".to_string(),
            codes.code(admin),
        ]);
        args.extend(extra.iter().map(|s| s.to_string()));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        mg_admin(&refs, "")
    };

    let requested = run(&["enrol", "request"], "alice", REQUEST_OLIVIA);
    assert!(requested.status.success(), "{}", stderr(&requested));
    let id = enrolment_id(&requested);

    let rejected = run(
        &["enrol", "reject"],
        "bob",
        &["--id", &id, "--reason", "not a known starter"],
    );
    assert!(rejected.status.success(), "{}", stderr(&rejected));
    assert!(
        stdout(&rejected).contains(&format!("rejected {id}"))
            && stdout(&rejected).contains("'olivia'"),
        "{}",
        stdout(&rejected)
    );
    assert!(stdout(&run(&["enrol", "list"], "bob", &[])).starts_with("0 enrolment(s)"));
    assert_eq!(login_status(&gateway, None), 401);

    // Nothing is left behind: olivia may be requested again.
    let again = run(&["enrol", "request"], "alice", REQUEST_OLIVIA);
    assert!(again.status.success(), "{}", stderr(&again));

    let _ = std::fs::remove_dir_all(&dir);
    for admin in ["alice", "bob"] {
        let _ = std::fs::remove_file(keystore(admin));
    }
}

/// GET the audit trail with `token`: the response body.
fn audit_trail(gateway: &str, token: &str) -> String {
    let addr = gateway.trim_start_matches("http://");
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "GET /api/v1/audit?limit=200 HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer \
         {token}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply
}

/// [AC-10.4] [VLD-ANB-MIDGARD-0008] Administration needs the one-time code as well as
/// the passkey: without one mg-admin stops before contacting the gateway, a wrong or
/// reset-away code is refused by it, and with the code the request goes through, the
/// assertion's user-verification flag signed truthfully. A passkey keystore of the
/// current format opens with no local gate, so the flag is false; once WP-50 wraps
/// the passkey under a passphrase it is true after the unwrap, and a wrong passphrase
/// stops mg-admin first (`a_wrong_passkey_passphrase_stops_before_the_gateway`).
#[test]
fn administration_needs_the_passphrase_and_a_one_time_code() {
    let (dir, keystore, codes) = two_administrators("mfa");
    let dir_arg = dir.display().to_string();

    // A lost authenticator: alice's secret is replaced, offline, and audited.
    let old = codes.secrets.borrow()["alice"].clone();
    let reset = mg_admin(
        &[
            "admin",
            "totp-reset",
            "--state-dir",
            &dir_arg,
            "--admin",
            "alice",
        ],
        "",
    );
    assert!(reset.status.success(), "{}", stderr(&reset));
    codes.learn("alice", &stdout(&reset));
    let unknown = mg_admin(
        &[
            "admin",
            "totp-reset",
            "--state-dir",
            &dir_arg,
            "--admin",
            "mallory",
        ],
        "",
    );
    assert_eq!(unknown.status.code(), Some(1), "{}", stderr(&unknown));

    // A principal who may read the audit trail.
    let reader = mg_admin(
        &[
            "user",
            "add",
            "--state-dir",
            &dir_arg,
            "--subject",
            "reader",
            "--roles",
            "operator",
            "--clearance",
            "internal",
            "--purposes",
            "operations",
        ],
        &format!("{PASSWORD}\n"),
    );
    assert!(reader.status.success(), "{}", stderr(&reader));

    let gateway = serve(&dir);
    let list = |code: Option<&str>| {
        let mut args = vec![
            "enrol".to_string(),
            "list".to_string(),
            "--gateway".to_string(),
            gateway.clone(),
            "--admin".to_string(),
            "alice".to_string(),
            "--keystore".to_string(),
            keystore("alice").display().to_string(),
        ];
        if let Some(code) = code {
            args.extend(["--totp".to_string(), code.to_string()]);
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        mg_admin(&refs, "")
    };

    // No code: a usage error before the gateway is contacted.
    let none = list(None);
    assert_eq!(none.status.code(), Some(2), "{}", stderr(&none));
    assert!(stderr(&none).contains("one-time code"), "{}", stderr(&none));
    // The gateway refuses a code from the secret the reset replaced.
    let stale = list(Some(&old.code_at(now())));
    assert_eq!(stale.status.code(), Some(1), "{}", stderr(&stale));
    // The gateway says only `unauthorised`; "second factor required" stays in its
    // log. [VLD-ANB-MIDGARD-0053]
    assert!(
        stderr(&stale).contains("the gateway refused (401): unauthorised"),
        "{}",
        stderr(&stale)
    );

    // The passkey and the current code: the request goes through.
    let mut args: Vec<String> = ["enrol", "request"].iter().map(|s| s.to_string()).collect();
    args.extend([
        "--gateway".to_string(),
        gateway.clone(),
        "--admin".to_string(),
        "alice".to_string(),
        "--keystore".to_string(),
        keystore("alice").display().to_string(),
        "--totp".to_string(),
        codes.code("alice"),
    ]);
    args.extend(REQUEST_OLIVIA.iter().map(|s| s.to_string()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let requested = mg_admin(&refs, "");
    assert!(requested.status.success(), "{}", stderr(&requested));
    let listed = list(Some(&codes.code("alice")));
    assert!(listed.status.success(), "{}", stderr(&listed));
    assert!(
        stdout(&listed).starts_with("1 enrolment(s)"),
        "{}",
        stdout(&listed)
    );

    // The ledger shows both factors, the flag as signed, the refusal and the reset.
    let (status, body) = post(
        &gateway,
        "/api/v1/login",
        &format!(r#"{{"username":"reader","password":"{PASSWORD}"}}"#),
    );
    assert_eq!(status, 200, "{body}");
    let token = body
        .split("\"token\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a token");
    let trail = audit_trail(&gateway, token);
    for shown in [
        // WP-50: `uv-claimed=true` once the passkey opens under its passphrase.
        "principal:olivia factors=totp+webauthn uv-claimed=false",
        r#""actor":"alice","action":"ADMIN refused (401)""#,
        r#""action":"ROTATE administrator totp""#,
    ] {
        assert!(trail.contains(shown), "{shown}: {trail}");
    }

    let _ = std::fs::remove_dir_all(&dir);
    for admin in ["alice", "bob"] {
        let _ = std::fs::remove_file(keystore(admin));
    }
}

/// [AC-10.4] [VLD-ANB-MIDGARD-0008] A wrong passkey passphrase stops mg-admin before it
/// contacts the gateway. Completes on merge with WP-50, which wraps the passkey
/// keystore under `--passkey-passphrase-file`; then fold this into
/// `administration_needs_the_passphrase_and_a_one_time_code`, write the passkeys with
/// a passphrase there and expect `uv-claimed=true`.
#[test]
#[ignore = "completes on merge with WP-50: the passkey keystore has no passphrase before it"]
fn a_wrong_passkey_passphrase_stops_before_the_gateway() {
    let (dir, keystore, codes) = two_administrators("passphrase");
    let wrong = dir.with_extension("wrong-passphrase");
    std::fs::write(&wrong, "not the passkey passphrase\n").unwrap();
    // Nothing listens here: reaching for the gateway would fail differently.
    let refused = mg_admin(
        &[
            "enrol",
            "list",
            "--gateway",
            "http://127.0.0.1:9",
            "--admin",
            "alice",
            "--keystore",
            &keystore("alice").display().to_string(),
            "--passkey-passphrase-file",
            &wrong.display().to_string(),
            "--totp",
            &codes.code("alice"),
        ],
        "",
    );
    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    assert!(
        stderr(&refused).contains("passkey keystore"),
        "{}",
        stderr(&refused)
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&wrong);
    for admin in ["alice", "bob"] {
        let _ = std::fs::remove_file(keystore(admin));
    }
}

/// The audit key `mg-admin init` exported, pinned as an operator would pin it.
/// [MG-SEC-05]
fn pin(state: &StateDir) -> audit_writer::PinnedKey {
    http_gateway::boot::read_audit_pin(&state.audit_trust_anchor()).unwrap()
}
