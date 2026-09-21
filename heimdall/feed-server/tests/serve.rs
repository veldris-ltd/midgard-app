//! `heimdall-feed` and `heimdall-source` end to end, as real processes on real sockets:
//! certified sources submit signed observations that fuse into one tracked entity; an
//! observation for another feed, one signed with an uncertified key, a submission by
//! an analyst and a read by a source are all refused and audited; the analyst reads
//! the fused picture; and after a hard kill the sealed picture and the ledger are
//! recovered and fusion continues on the recovered tracks.
//! [FR-H-1] [FR-H-3] [NFR-PQ-3] [RC-13] [NFR-SEC-1]
#![cfg(feature = "backend-fips")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use attest::Keystore;
use heimdall_feed_gateway::{encode_signed_observation, sign_observation};
use heimdall_feed_server::{outcome, INGEST};
use heimdall_picture::{FeedId, Observation};
use iam::{Idp, IDP_SIG_ALG};
use policy::Role;
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::{OsRng, SignatureScheme};
use service_runtime::json::{parse, Json};
use service_runtime::{client, ServiceIdentity};

const POLICY: &str = r#"{
  "version": 1,
  "rules": [
    { "id": "source-channel", "role": "feed-source", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "source-ingest", "role": "feed-source", "action": "Ingest",
      "resource": "*", "effect": "permit" },
    { "id": "analyst-channel", "role": "picture-reader", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "analyst-read", "role": "picture-reader", "action": "ReadPicture",
      "resource": "*", "effect": "permit" }
  ]
}"#;

const PURPOSE: &str = "situational-awareness";

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "heimdall-feed-server-{tag}-{}-{uniq}",
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
    entities: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(identity: &Path, state: &Path) -> Running {
    let mut child = Command::new(env!("CARGO_BIN_EXE_heimdall-feed"))
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
        .expect("heimdall-feed starts");
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
                panic!("heimdall-feed exited before serving: {stderr}");
            }
        }
    };
    std::thread::spawn(move || lines.for_each(drop));
    Running {
        child,
        mesh: field(&serving, "mesh").expect("mesh address"),
        health: field(&serving, "health").expect("health address"),
        recovered: field(&serving, "recovered_audit_entries").expect("recovered count"),
        entities: field(&serving, "entities").expect("entity count"),
    }
}

fn source(dir: &Path, service: &str, addr: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_heimdall-source"))
        .args([
            "--identity-dir",
            &dir.display().to_string(),
            "--service",
            service,
            "--addr",
            addr,
        ])
        .args(args)
        .output()
        .expect("heimdall-source runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn picture(dir: &Path, addr: &str) -> Json {
    let out = source(dir, "analyst", addr, &["--picture"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    parse(text(&out.stdout).trim()).expect("the picture is JSON")
}

fn entity<'a>(picture: &'a Json, id: &str) -> &'a Json {
    match picture.get("entities") {
        Some(Json::Arr(items)) => items
            .iter()
            .find(|e| e.str_field("id") == Some(id))
            .unwrap_or_else(|| panic!("{id} is not in {}", picture.encode())),
        _ => panic!("no entities in {}", picture.encode()),
    }
}

fn get(addr: &str, path: &str) -> u16 {
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: probe\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap()
}

#[test]
fn sources_feed_the_picture_analysts_read_it_and_it_survives_a_restart() {
    let root = scratch("e2e");
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let server_identity = root.join("server");
    let clients = root.join("clients");
    let state = root.join("state");
    issue(&idp, &server_identity, "heimdall-feed", &["feed-gateway"]);
    issue(&idp, &clients, "radar-1", &["feed-source"]);
    issue(&idp, &clients, "radar-2", &["feed-source"]);
    issue(&idp, &clients, "analyst", &["picture-reader"]);
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("policy.json"), POLICY).unwrap();

    let server = start(&server_identity, &state);
    assert_eq!(server.recovered, "0");
    assert_eq!(get(&server.health, "/readyz"), 200);

    let observe = |service: &str, at: &str, attrs: &str, extra: &[&str]| {
        let mut args = vec![
            "--kind",
            "air-track",
            "--entity",
            "AC-1",
            "--observed-at",
            at,
            "--attrs",
            attrs,
        ];
        args.extend_from_slice(extra);
        source(&clients, service, &server.mesh, &args)
    };

    let first = observe("radar-1", "100", "speed=420,callsign=ALPHA1", &[]);
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert!(
        text(&first.stdout).contains("fused into entity AC-1 at audit sequence 0"),
        "{}",
        text(&first.stdout)
    );
    let second = observe("radar-2", "110", "speed=430", &[]);
    assert!(second.status.success(), "{}", text(&second.stderr));

    let impersonation = observe("radar-2", "111", "speed=999", &["--feed-id", "radar-1"]);
    assert_eq!(impersonation.status.code(), Some(1));
    assert!(
        text(&impersonation.stderr).contains("only its own feed"),
        "{}",
        text(&impersonation.stderr)
    );

    // Signed by a key no certificate binds, sent over radar-1's own channel.
    let (_, rogue_key) = FipsSig::new(IDP_SIG_ALG).keypair(&mut OsRng).unwrap();
    let forged = sign_observation(
        &FipsSig::new(IDP_SIG_ALG),
        &rogue_key,
        Observation::new("forged", FeedId("radar-1".into()), "air-track", "AC-1", 112),
    )
    .unwrap();
    let radar_1 = ServiceIdentity::load(&clients, "radar-1").unwrap();
    let response = client::call(
        &radar_1,
        &server.mesh,
        PURPOSE,
        INGEST,
        &encode_signed_observation(&forged),
    )
    .unwrap();
    assert_eq!(response[0], outcome::DENIED);
    assert!(
        text(&response[1..]).contains("rejected"),
        "{}",
        text(&response[1..])
    );

    let analyst_ingest = observe("analyst", "113", "speed=1", &[]);
    assert_eq!(
        analyst_ingest.status.code(),
        Some(1),
        "an analyst is not a source"
    );
    let source_read = source(&clients, "radar-1", &server.mesh, &["--picture"]);
    assert_eq!(
        source_read.status.code(),
        Some(1),
        "a source cannot read the picture"
    );

    let fused = picture(&clients, &server.mesh);
    let ac_1 = entity(&fused, "AC-1");
    assert_eq!(
        ac_1.get("observations"),
        Some(&Json::Int(2)),
        "{}",
        fused.encode()
    );
    assert_eq!(ac_1.get("lastSeen"), Some(&Json::Int(110)));
    assert_eq!(
        ac_1.get("sources"),
        Some(&Json::Arr(vec![Json::s("radar-1"), Json::s("radar-2")]))
    );
    let attributes = ac_1.get("attributes").expect("attributes");
    assert_eq!(attributes.get("speed"), Some(&Json::Int(430)));

    // A hard kill. Audited so far: two submissions, the impersonation and the forgery
    // (each a decision and a refusal), the analyst's submission, the source's read and
    // the analyst's read.
    drop(server);
    let server = start(&server_identity, &state);
    assert_eq!(server.recovered, "9");
    assert_eq!(server.entities, "1");

    let third = source(
        &clients,
        "radar-1",
        &server.mesh,
        &[
            "--kind",
            "air-track",
            "--entity",
            "AC-1",
            "--observed-at",
            "120",
        ],
    );
    assert!(third.status.success(), "{}", text(&third.stderr));
    assert!(text(&third.stdout).contains("audit sequence 9"));
    let fused = picture(&clients, &server.mesh);
    assert_eq!(
        entity(&fused, "AC-1").get("observations"),
        Some(&Json::Int(3)),
        "fusion continues on the recovered track"
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}
