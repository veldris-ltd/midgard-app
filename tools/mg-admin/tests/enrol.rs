//! `mg-admin admin add` and `mg-admin enrol` end to end: the real binary against a real
//! gateway on a socket. Two IAM administrators get passkeys; one requests an enrolment,
//! cannot approve it herself, and the other approves it — after which the principal
//! logs in. A gateway that is not on loopback is refused. [FR-AD-5] [RC-29] [RC-30]

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
    let served = open_state_dir(&StateDir::new(dir), Vec::new()).expect("the enclosure boots");
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

fn login_status(gateway: &str) -> u16 {
    let addr = gateway.trim_start_matches("http://");
    let body = format!(r#"{{"username":"olivia","password":"{PASSWORD}"}}"#);
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "POST /api/v1/login HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status")
}

#[test]
fn two_administrators_enrol_a_principal_through_a_running_gateway() {
    let dir = scratch("dual");
    let dir_arg = dir.display().to_string();
    let init = mg_admin(&["init", "--state-dir", &dir_arg], "");
    assert!(init.status.success(), "{}", stderr(&init));

    let keystore = |admin: &str| dir.with_extension(format!("{admin}.passkey"));
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
    }
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
    let run = |command: &[&str], admin: &str, extra: &[&str], stdin: &str| {
        let mut args: Vec<String> = command.iter().map(|s| s.to_string()).collect();
        args.extend(as_admin(admin));
        args.extend(extra.iter().map(|s| s.to_string()));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        mg_admin(&refs, stdin)
    };

    let requested = run(
        &["enrol", "request"],
        "alice",
        &[
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
        ],
        &format!("{PASSWORD}\n"),
    );
    assert!(requested.status.success(), "{}", stderr(&requested));
    let text = stdout(&requested);
    let id = text
        .split_whitespace()
        .find(|w| w.starts_with("enrol-olivia-"))
        .unwrap_or_else(|| panic!("an enrolment id in: {text}"))
        .to_string();
    assert_eq!(login_status(&gateway), 401, "not enrolled until approved");

    let own = run(&["enrol", "approve"], "alice", &["--id", &id], "");
    assert_eq!(own.status.code(), Some(1));
    assert!(
        stderr(&own).contains("SelfApprovalDenied"),
        "{}",
        stderr(&own)
    );

    let listed = run(&["enrol", "list"], "bob", &[], "");
    assert!(listed.status.success(), "{}", stderr(&listed));
    assert!(
        stdout(&listed).contains(&id) && stdout(&listed).contains("requested-by=alice"),
        "{}",
        stdout(&listed)
    );

    let approved = run(&["enrol", "approve"], "bob", &["--id", &id], "");
    assert!(approved.status.success(), "{}", stderr(&approved));
    assert!(
        stdout(&approved).contains("'olivia' is enrolled"),
        "{}",
        stdout(&approved)
    );
    assert!(stdout(&run(&["enrol", "list"], "bob", &[], "")).starts_with("0 enrolment(s)"));
    assert_eq!(login_status(&gateway), 200, "olivia logs in once approved");

    // Administration never leaves loopback.
    let mut remote: Vec<String> = ["enrol", "list"].iter().map(|s| s.to_string()).collect();
    remote.extend(as_admin("bob"));
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
