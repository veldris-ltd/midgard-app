//! The artefact registry in `bifrost-server`, end to end as a real process on real
//! sockets: a release manager uploads a multi-MB signed build in chunks and the gate
//! admits it; a forged signature, a mismatched SBOM, an unknown signer and tampered bytes
//! are refused, recorded with their reason and listed as rejected; a download is
//! re-verified; an identity without `RegisterArtefact` is refused; reads are split, so an
//! agent may download but not list or re-verify, and an operator the reverse; and after a
//! hard kill
//! the registry comes back, sealed — a flipped byte at rest stops the start.
//! [FR-B-1] [FR-B-4] [NFR-PQ-3] [RC-12] [NFR-SEC-1]
#![cfg(feature = "backend-fips")]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use attest::Keystore;
use bifrost_artefact::{build_and_sign, generate_sbom, SbomComponent, SignedArtefact};
use bifrost_fleet_server::client::{self, CallError};
use bifrost_fleet_server::outcome;
use iam::{Idp, IDP_SIG_ALG};
use policy::Role;
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::hash::sha256;
use pqc_agility::{OsRng, SignatureScheme, SigningKey, VerifyingKey};
use service_runtime::client::Session;
use service_runtime::json::Json;
use service_runtime::ServiceIdentity;

const POLICY: &str = r#"{
  "version": 1,
  "rules": [
    { "id": "release-channel", "role": "release-manager", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "release-register", "role": "release-manager", "action": "RegisterArtefact",
      "resource": "*", "effect": "permit" },
    { "id": "release-read", "role": "release-manager", "action": "ReadArtefacts",
      "resource": "*", "effect": "permit" },
    { "id": "release-verify", "role": "release-manager", "action": "VerifyArtefact",
      "resource": "*", "effect": "permit" },
    { "id": "agent-channel", "role": "fleet-agent", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "agent-download", "role": "fleet-agent", "action": "DownloadArtefact",
      "resource": "*", "effect": "permit" },
    { "id": "operator-channel", "role": "fleet-operator", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "operator-read", "role": "fleet-operator", "action": "ReadFleet",
      "resource": "*", "effect": "permit" },
    { "id": "operator-artefacts", "role": "fleet-operator", "action": "ReadArtefacts",
      "resource": "*", "effect": "permit" },
    { "id": "operator-verify", "role": "fleet-operator", "action": "VerifyArtefact",
      "resource": "*", "effect": "permit" }
  ]
}"#;

const CHANNELS: &str = r#"{ "version": 1, "channels": { "stable": "0.1.0" } }"#;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "bifrost-registry-e2e-{tag}-{}-{uniq}",
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

fn field(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let len = line[start..].find('"')?;
    Some(line[start..start + len].to_string())
}

struct Running {
    child: Child,
    mesh: String,
    serving: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn(identity: &Path, state: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_bifrost-server"))
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
        .expect("bifrost-server starts")
}

/// Start the server, or return what it said when it would not start.
fn try_start(identity: &Path, state: &Path) -> Result<Running, String> {
    let mut child = spawn(identity, state);
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
                let _ = child.wait();
                return Err(stderr);
            }
        }
    };
    std::thread::spawn(move || lines.for_each(drop));
    Ok(Running {
        child,
        mesh: field(&serving, "mesh").expect("mesh address"),
        serving,
    })
}

fn start(identity: &Path, state: &Path) -> Running {
    try_start(identity, state).unwrap_or_else(|e| panic!("bifrost-server did not start: {e}"))
}

fn session(clients: &Path, service: &str, addr: &str) -> Session {
    let identity = ServiceIdentity::load(clients, service).unwrap();
    Session::open(&identity, addr, "fleet-management").unwrap()
}

fn keys() -> (VerifyingKey, SigningKey) {
    FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
        .keypair(&mut OsRng)
        .unwrap()
}

fn build(sk: &SigningKey, version: &str, payload: Vec<u8>) -> SignedArtefact {
    let sbom = generate_sbom(
        "forge-worker",
        version,
        "service",
        vec![
            SbomComponent::new("pqc-agility", "0.1.0"),
            SbomComponent::new("forge-engine", "0.1.0"),
        ],
    );
    build_and_sign("forge-worker", version, "service", payload, sbom, sk)
}

/// A ~3 MB "binary" that does not repeat, so a misplaced chunk would show.
fn binary(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).max(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn refused(result: Result<Json, CallError>, reason: &str) {
    match result {
        Err(CallError {
            code: Some(outcome::REJECTED),
            message,
        }) => assert!(message.contains(reason), "{message}"),
        other => panic!("expected a {reason} refusal, got {other:?}"),
    }
}

fn denied<T: std::fmt::Debug>(result: Result<T, CallError>, action: &str) {
    match result {
        Err(CallError {
            code: Some(outcome::DENIED),
            message,
        }) => assert!(message.contains(action), "{message}"),
        other => panic!("expected {action} to be denied, got {other:?}"),
    }
}

fn artefacts(listing: &Json) -> Vec<(String, String, String)> {
    match listing.get("artefacts") {
        Some(Json::Arr(items)) => items
            .iter()
            .map(|i| {
                (
                    i.str_field("version").unwrap().to_string(),
                    i.str_field("status").unwrap().to_string(),
                    i.str_field("reason").unwrap_or("").to_string(),
                )
            })
            .collect(),
        _ => panic!("no artefacts in {}", listing.encode()),
    }
}

#[test]
fn the_registry_admits_only_verified_builds_and_keeps_them_sealed_across_a_restart() {
    let root = scratch("e2e");
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let server_identity = root.join("server");
    let clients = root.join("clients");
    let state = root.join("state");
    issue(&idp, &server_identity, "bifrost-server", &["fleet-manager"]);
    issue(&idp, &clients, "release-ci", &["release-manager"]);
    issue(&idp, &clients, "ops-console", &["fleet-operator"]);
    issue(&idp, &clients, "fleet-agent-1", &["fleet-agent"]);
    std::fs::create_dir_all(state.join("trust/release-signers")).unwrap();
    std::fs::write(state.join("policy.json"), POLICY).unwrap();
    std::fs::write(state.join("channels.json"), CHANNELS).unwrap();
    let (vk, sk) = keys();
    std::fs::write(
        state.join("trust/release-signers/release.pub"),
        format!("ML-DSA-65 release-1 {}\n", attest::hex::hex(vk.as_bytes())),
    )
    .unwrap();

    let server = start(&server_identity, &state);
    assert_eq!(
        field(&server.serving, "release_signers").as_deref(),
        Some("release-1")
    );
    let mut ci = session(&clients, "release-ci", &server.mesh);

    // A multi-MB build, in seven chunks, through the gate.
    let good = build(&sk, "0.1.0", binary(3 * 1024 * 1024 + 12_345, 1));
    let listed = client::register(&mut ci, &good, "release-1").expect("a good build registers");
    assert_eq!(listed.str_field("status"), Some("verified"));
    assert_eq!(listed.str_field("registeredBy"), Some("release-ci"));
    assert_eq!(
        listed.get("size").and_then(Json::as_i64),
        Some(good.payload.len() as i64)
    );

    // Each way a build can be wrong is refused for its own reason.
    let (_, other_sk) = keys();
    let forged = build(&other_sk, "0.1.1", binary(1000, 2));
    refused(
        client::register(&mut ci, &forged, "release-1"),
        "bad-signature",
    );
    let mut bad_sbom = build(&sk, "0.1.2", binary(1000, 3));
    bad_sbom.sbom = bad_sbom.sbom.replace("forge-engine", "forge-engine-evil");
    refused(
        client::register(&mut ci, &bad_sbom, "release-1"),
        "sbom-mismatch",
    );
    let unknown = build(&other_sk, "0.1.3", binary(1000, 4));
    refused(
        client::register(&mut ci, &unknown, "release-9"),
        "unknown-signer",
    );
    let mut tampered = build(&sk, "0.1.4", binary(700_000, 5));
    tampered.payload[650_000] ^= 0x40;
    refused(
        client::register(&mut ci, &tampered, "release-1"),
        "digest-mismatch",
    );
    // The same id and version again is a conflict, not a second copy.
    match client::register(&mut ci, &good, "release-1") {
        Err(CallError {
            code: Some(outcome::CONFLICT),
            ..
        }) => {}
        other => panic!("{other:?}"),
    }

    let listing = client::list(&mut ci).unwrap();
    let expected = vec![
        ("0.1.0".to_string(), "verified".to_string(), String::new()),
        ("0.1.1".into(), "rejected".into(), "bad-signature".into()),
        ("0.1.2".into(), "rejected".into(), "sbom-mismatch".into()),
        ("0.1.3".into(), "rejected".into(), "unknown-signer".into()),
        ("0.1.4".into(), "rejected".into(), "digest-mismatch".into()),
    ];
    assert_eq!(artefacts(&listing), expected);

    // An operator may read the registry but not register: the policy refuses it.
    let mut ops = session(&clients, "ops-console", &server.mesh);
    match client::register(&mut ops, &build(&sk, "0.2.0", binary(10, 6)), "release-1") {
        Err(CallError {
            code: Some(outcome::DENIED),
            message,
        }) => assert!(message.contains("RegisterArtefact"), "{message}"),
        other => panic!("an operator registered: {other:?}"),
    }
    // Reads are split: an operator lists and re-verifies, but does not download; an agent
    // downloads, but does not list or re-verify.
    denied(
        client::download(&mut ops, "forge-worker", "0.1.0"),
        "DownloadArtefact",
    );
    let mut agent = session(&clients, "fleet-agent-1", &server.mesh);
    denied(client::list(&mut agent), "ReadArtefacts");
    denied(
        client::verify(&mut agent, "forge-worker", "0.1.0"),
        "VerifyArtefact",
    );
    let downloaded = client::download(&mut agent, "forge-worker", "0.1.0").unwrap();
    assert_eq!(downloaded.signer, "release-1");
    assert_eq!(downloaded.artefact.payload, good.payload);
    assert_eq!(downloaded.artefact.sbom, good.sbom);
    bifrost_artefact::verify(&vk, &downloaded.artefact).expect("the download verifies");
    assert_eq!(
        client::verify(&mut ops, "forge-worker", "0.1.0")
            .unwrap()
            .str_field("status"),
        Some("verified")
    );
    // A second download is served from the same verified copy, as the first was.
    let twice = client::download(&mut agent, "forge-worker", "0.1.0").unwrap();
    assert_eq!(twice.artefact.payload, good.payload);
    match client::download(&mut agent, "forge-worker", "9.9.9") {
        Err(CallError {
            code: Some(outcome::NOT_FOUND),
            ..
        }) => {}
        other => panic!("{other:?}"),
    }
    drop((ci, ops, agent));

    // A hard kill: verified artefacts and refusals come back, still sealed.
    drop(server);
    let server = start(&server_identity, &state);
    assert_eq!(field(&server.serving, "artefacts").as_deref(), Some("1"));
    assert_eq!(field(&server.serving, "rejected").as_deref(), Some("4"));
    let mut ops = session(&clients, "ops-console", &server.mesh);
    assert_eq!(artefacts(&client::list(&mut ops).unwrap()), expected);
    let mut anchors = attest::TrustAnchors::new();
    anchors.insert("release-1", vk.clone());
    let mut agent = session(&clients, "fleet-agent-1", &server.mesh);
    let again = client::download_verified(&mut agent, "forge-worker", "0.1.0", &anchors).unwrap();
    assert_eq!(again.artefact.payload, good.payload);
    // The caller's own anchors decide: a caller that does not trust release-1 refuses.
    assert!(client::download_verified(
        &mut agent,
        "forge-worker",
        "0.1.0",
        &attest::TrustAnchors::new()
    )
    .unwrap_err()
    .message
    .contains("not one this caller trusts"));

    // The listing comes a page at a time, with a cursor.
    let page = client::call(
        &mut ops,
        bifrost_fleet_server::LIST_ARTEFACTS,
        &Json::obj(vec![("limit", Json::Int(2))]),
    )
    .unwrap();
    match page.get("artefacts") {
        Some(Json::Arr(items)) => assert_eq!(items.len(), 2),
        other => panic!("{other:?}"),
    }
    assert!(page.str_field("next").is_some(), "{}", page.encode());

    // An SBOM near the 512 KiB cap, full of characters JSON escapes, goes up and comes
    // back: it travels as base64, in its own slices.
    let mut ci = session(&clients, "release-ci", &server.mesh);
    let components: Vec<SbomComponent> = (0..2600)
        .map(|i| SbomComponent::new(format!("crate-\"quoted\"-{i:05}"), "1.0.0"))
        .collect();
    let big_sbom = generate_sbom("big-sbom", "1.0.0", "service", components);
    assert!(
        big_sbom.len() > 400 * 1024 && big_sbom.len() <= 512 * 1024,
        "{}",
        big_sbom.len()
    );
    let big = build_and_sign(
        "big-sbom",
        "1.0.0",
        "service",
        binary(5000, 7),
        big_sbom,
        &sk,
    );
    client::register(&mut ci, &big, "release-1").expect("a large SBOM registers");
    let got = client::download_verified(&mut agent, "big-sbom", "1.0.0", &anchors).unwrap();
    assert_eq!(got.artefact.sbom, big.sbom);

    // A copy that goes bad while the server runs is refused, audited, and marked: it
    // is never served again, even once restored.
    let big_file = state.join("registry/artefacts").join(format!(
        "{}.sealed",
        attest::hex::hex(&sha256(b"big-sbom\x001.0.0"))
    ));
    let original = std::fs::read(&big_file).unwrap();
    let mut spoiled = original.clone();
    spoiled[100] ^= 1;
    std::fs::write(&big_file, &spoiled).unwrap();
    match client::verify(&mut ops, "big-sbom", "1.0.0") {
        Err(CallError {
            code: Some(outcome::REJECTED),
            message,
        }) => {
            assert!(message.contains("will not be served"), "{message}");
            assert!(
                !message.contains("not audited"),
                "the failure is audited: {message}"
            );
        }
        other => panic!("{other:?}"),
    }
    std::fs::write(&big_file, &original).unwrap();
    assert_eq!(
        client::download(&mut agent, "big-sbom", "1.0.0")
            .unwrap_err()
            .code,
        Some(outcome::REJECTED)
    );
    let listing = client::list(&mut ops).unwrap();
    let big_listed = match listing.get("artefacts") {
        Some(Json::Arr(items)) => items
            .iter()
            .find(|i| i.str_field("id") == Some("big-sbom"))
            .cloned()
            .unwrap(),
        _ => panic!(),
    };
    assert_eq!(big_listed.str_field("status"), Some("unverifiable"));
    drop((ci, ops, agent));
    drop(server);

    // Nothing at rest is in the clear, and a flipped byte stops the start.
    let good_file = state.join("registry/artefacts").join(format!(
        "{}.sealed",
        attest::hex::hex(&sha256(b"forge-worker\x000.1.0"))
    ));
    let sealed: Vec<PathBuf> = std::fs::read_dir(state.join("registry/artefacts"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(sealed.len(), 2, "rejected builds leave no bytes behind");
    let mut bytes = std::fs::read(&good_file).unwrap();
    assert!(!bytes
        .windows(64)
        .any(|w| w == &good.payload[1_000_000..1_000_064]));
    let journal = std::fs::read(state.join("registry/rejected.journal")).unwrap();
    assert!(!journal.windows(13).any(|w| w == b"bad-signature"));
    let mid = bytes.len() / 2;
    bytes[mid] ^= 1;
    std::fs::write(&good_file, bytes).unwrap();
    let err = try_start(&server_identity, &state)
        .err()
        .expect("a tampered registry does not start");
    assert!(err.contains("does not unseal"), "{err}");

    let _ = std::fs::remove_dir_all(&root);
}
