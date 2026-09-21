//! `bifrost-fleet` and `bifrost-agent` end to end, as real processes on real sockets:
//! agents check in over PQ-mTLS and every request is audited; an agent reporting for
//! another installation, an agent reading the fleet and an operator checking in are
//! refused; the operator sees each installation's version and liveness, the drift and
//! convergence; and after a hard kill the service recovers its check-ins and ledger.
//! [FR-B-3] [RC-12] [RC-17] [NFR-SEC-1]
#![cfg(feature = "backend-fips")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use attest::Keystore;
use iam::{Idp, IDP_SIG_ALG};
use policy::Role;
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
use pqc_agility::OsRng;
use service_runtime::json::{parse, Json};

const POLICY: &str = r#"{
  "version": 1,
  "rules": [
    { "id": "agent-channel", "role": "fleet-agent", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "agent-check-in", "role": "fleet-agent", "action": "CheckIn",
      "resource": "*", "effect": "permit" },
    { "id": "operator-channel", "role": "fleet-operator", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "operator-read", "role": "fleet-operator", "action": "ReadFleet",
      "resource": "*", "effect": "permit" }
  ]
}"#;

const CHANNELS: &str = r#"{ "version": 1, "channels": { "stable": "1.5.0" } }"#;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "bifrost-fleet-server-{tag}-{}-{uniq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What `mg-admin mesh issue` writes, for `service` under `idp`.
fn issue(idp: &Idp, dir: &Path, service: &str, roles: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    let key = Keystore::generate(service, IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(&format!("{service}-audit"), IDP_SIG_ALG, &mut OsRng).unwrap();
    let cert = idp
        .certify(&WorkloadIdentity {
            id: ServiceId(service.into()),
            roles: roles.iter().map(|r| Role(r.to_string())).collect(),
            sig_alg: IDP_SIG_ALG,
            verifying_key: key.verifying_key.clone(),
        })
        .unwrap();
    key.write(&dir.join(format!("{service}.keystore"))).unwrap();
    audit
        .write(&dir.join(format!("{service}.audit.keystore")))
        .unwrap();
    std::fs::write(dir.join(format!("{service}.cert")), encode_cert(&cert)).unwrap();
    std::fs::write(dir.join("mesh.anchor"), encode_anchor(&idp.trust_anchor())).unwrap();
}

/// A field of a JSON log line (the sink writes every field as a string).
fn field(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let len = line[start..].find('"')?;
    Some(line[start..start + len].to_string())
}

struct Running {
    child: Child,
    mesh: String,
    health: String,
    recovered: String,
    installations: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(identity: &Path, state: &Path) -> Running {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bifrost-fleet"))
        .args([
            "--identity-dir",
            &identity.display().to_string(),
            "--state-dir",
            &state.display().to_string(),
            "--bind",
            "127.0.0.1:0",
            "--health-bind",
            "127.0.0.1:0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("bifrost-fleet starts");
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let serving = loop {
        match lines.next() {
            Some(Ok(line)) if line.contains("\"message\":\"serving\"") => break line,
            Some(_) => continue,
            None => {
                let mut stderr = String::new();
                child
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut stderr)
                    .unwrap();
                panic!("bifrost-fleet exited before serving: {stderr}");
            }
        }
    };
    std::thread::spawn(move || lines.for_each(drop));
    Running {
        child,
        mesh: field(&serving, "mesh").expect("mesh address"),
        health: field(&serving, "health").expect("health address"),
        recovered: field(&serving, "recovered_audit_entries").expect("recovered count"),
        installations: field(&serving, "installations").expect("installation count"),
    }
}

fn agent(clients: &Path, service: &str, addr: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bifrost-agent"))
        .args([
            "--identity-dir",
            &clients.display().to_string(),
            "--service",
            service,
            "--addr",
            addr,
        ])
        .args(args)
        .output()
        .expect("bifrost-agent runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn fleet_status(clients: &Path, addr: &str) -> Json {
    let out = agent(clients, "ops-console", addr, &["--status"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    parse(text(&out.stdout).trim()).expect("the fleet is JSON")
}

fn installation<'a>(fleet: &'a Json, id: &str) -> &'a Json {
    match fleet.get("installations") {
        Some(Json::Arr(items)) => items
            .iter()
            .find(|i| i.str_field("id") == Some(id))
            .unwrap_or_else(|| panic!("{id} is not in {}", fleet.encode())),
        _ => panic!("no installations in {}", fleet.encode()),
    }
}

fn get(addr: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: probe\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap();
    (status, response)
}

#[test]
fn agents_check_in_operators_read_the_fleet_and_it_survives_a_restart() {
    let root = scratch("e2e");
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let server_identity = root.join("server");
    let clients = root.join("clients");
    let state = root.join("state");
    issue(&idp, &server_identity, "bifrost-fleet", &["fleet-manager"]);
    issue(&idp, &clients, "host-a", &["fleet-agent"]);
    issue(&idp, &clients, "host-b", &["fleet-agent"]);
    issue(&idp, &clients, "ops-console", &["fleet-operator"]);
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("policy.json"), POLICY).unwrap();
    std::fs::write(state.join("channels.json"), CHANNELS).unwrap();

    let server = start(&server_identity, &state);
    assert_eq!(server.recovered, "0");
    assert_eq!(get(&server.health, "/readyz").0, 200);

    let a = agent(
        &clients,
        "host-a",
        &server.mesh,
        &["--channel", "stable", "--version", "1.4.0"],
    );
    assert!(a.status.success(), "{}", text(&a.stderr));
    assert!(
        text(&a.stdout).contains("recorded at audit sequence 0"),
        "{}",
        text(&a.stdout)
    );
    let b = agent(
        &clients,
        "host-b",
        &server.mesh,
        &["--channel", "stable", "--version", "1.5.0", "--unhealthy"],
    );
    assert!(b.status.success(), "{}", text(&b.stderr));

    let impersonation = agent(
        &clients,
        "host-b",
        &server.mesh,
        &[
            "--channel",
            "stable",
            "--version",
            "1.5.0",
            "--install-id",
            "host-a",
        ],
    );
    assert_eq!(impersonation.status.code(), Some(1));
    assert!(
        text(&impersonation.stderr).contains("only for its own installation"),
        "{}",
        text(&impersonation.stderr)
    );
    let agent_read = agent(&clients, "host-a", &server.mesh, &["--status"]);
    assert_eq!(
        agent_read.status.code(),
        Some(1),
        "an agent cannot read the fleet"
    );
    assert!(text(&agent_read.stderr).contains("denied"));
    let operator_check_in = agent(
        &clients,
        "ops-console",
        &server.mesh,
        &["--channel", "stable", "--version", "1.5.0"],
    );
    assert_eq!(
        operator_check_in.status.code(),
        Some(1),
        "an operator is not an agent"
    );

    let fleet = fleet_status(&clients, &server.mesh);
    let host_a = installation(&fleet, "host-a");
    assert_eq!(host_a.str_field("version"), Some("1.4.0"));
    assert_eq!(host_a.str_field("status"), Some("live"));
    assert_eq!(
        installation(&fleet, "host-b").str_field("status"),
        Some("unhealthy")
    );
    match fleet.get("drift") {
        Some(Json::Arr(drift)) => {
            assert_eq!(drift.len(), 1, "{}", fleet.encode());
            assert_eq!(drift[0].str_field("installId"), Some("host-a"));
            assert_eq!(drift[0].str_field("to"), Some("1.5.0"));
        }
        _ => panic!("no drift in {}", fleet.encode()),
    }
    assert_eq!(fleet.get("converged"), Some(&Json::Bool(false)));
    let (_, metrics) = get(&server.health, "/metrics");
    assert!(metrics.contains("fleet_requests_total{"), "{metrics}");
    assert!(metrics.contains("outcome=\"denied\""), "{metrics}");

    // A hard kill. Audited so far: two check-ins, the impersonation (its decision and
    // its refusal), the agent's read, the operator's check-in and the operator's read.
    drop(server);
    let server = start(&server_identity, &state);
    assert_eq!(server.recovered, "7");
    assert_eq!(server.installations, "2");
    let fleet = fleet_status(&clients, &server.mesh);
    assert_eq!(
        installation(&fleet, "host-b").str_field("status"),
        Some("unhealthy"),
        "check-ins, not just installations, are recovered"
    );

    let upgraded = agent(
        &clients,
        "host-a",
        &server.mesh,
        &["--channel", "stable", "--version", "1.5.0"],
    );
    assert!(upgraded.status.success(), "{}", text(&upgraded.stderr));
    assert!(text(&upgraded.stdout).contains("recorded at audit sequence 8"));
    let fleet = fleet_status(&clients, &server.mesh);
    assert_eq!(fleet.get("converged"), Some(&Json::Bool(true)));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}
