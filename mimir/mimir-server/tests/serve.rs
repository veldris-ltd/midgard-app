//! `mimir-server` and `mimir-probe` end to end, as real processes on real sockets: a
//! certified consumer's call is served over PQ-mTLS and audited; a consumer whose roles
//! policy does not permit, and one certified by another issuer, are refused and counted;
//! the service reports ready and holds its state directory; and after a hard kill it
//! restarts on the same ledger, recovering and continuing its audit sequence.
//! [FR-MI-8] [NFR-SEC-1] [RC-33] [RC-09] [AC-MI-8]
#![cfg(feature = "backend-fips")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use attest::Keystore;
use iam::{Idp, IDP_SIG_ALG};
use policy::Role;
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
use pqc_agility::OsRng;

const POLICY: &str = r#"{
  "version": 1,
  "rules": [
    { "id": "consumer-channel", "role": "inference-consumer", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "consumer-infer", "role": "inference-consumer", "action": "Infer",
      "resource": "*", "effect": "permit" }
  ]
}"#;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir =
        std::env::temp_dir().join(format!("mimir-server-{tag}-{}-{uniq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What `mg-admin mesh issue` writes, for `service` under `idp`.
fn issue(idp: &Idp, dir: &Path, service: &str, roles: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    let key = Keystore::generate(service, IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(&format!("{service}-audit"), IDP_SIG_ALG, &mut OsRng).unwrap();
    let cert = idp
        .certify(
            &WorkloadIdentity {
                id: ServiceId(service.into()),
                roles: roles.iter().map(|r| Role(r.to_string())).collect(),
                sig_alg: IDP_SIG_ALG,
                verifying_key: key.verifying_key.clone(),
            },
            pq_mtls::CertTerms::new(
                1,
                0,
                u64::MAX,
                pq_mtls::Posture::at(policy::ClassificationLevel::TOP_SECRET),
            ),
        )
        .unwrap();
    key.write(&dir.join(format!("{service}.keystore"))).unwrap();
    audit
        .write(&dir.join(format!("{service}.audit.keystore")))
        .unwrap();
    std::fs::write(dir.join(format!("{service}.cert")), encode_cert(&cert)).unwrap();
    std::fs::write(dir.join("mesh.anchor"), encode_anchor(&idp.trust_anchor())).unwrap();
    let crl = idp.sign_crl(1, 0, Default::default()).unwrap();
    std::fs::write(dir.join("mesh.crl"), pq_mtls::encode_crl(&crl)).unwrap();
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
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn server_args<'a>(identity: &'a str, state: &'a str) -> Vec<&'a str> {
    vec![
        "--identity-dir",
        identity,
        "--state-dir",
        state,
        "--purposes",
        "investigation",
        "--bind",
        "127.0.0.1:0",
        "--health-bind",
        "127.0.0.1:0",
    ]
}

fn start(identity: &Path, state: &Path) -> Running {
    start_with(identity, state, &[])
}

fn start_with(identity: &Path, state: &Path, extra: &[&str]) -> Running {
    let (identity, state) = (identity.display().to_string(), state.display().to_string());
    let mut child = Command::new(env!("CARGO_BIN_EXE_mimir-server"))
        .args(server_args(&identity, &state))
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mimir-server starts");
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
                panic!("mimir-server exited before serving: {stderr}");
            }
        }
    };
    // Keep draining the connection log so the pipe never fills.
    std::thread::spawn(move || lines.for_each(drop));
    Running {
        child,
        mesh: field(&serving, "mesh").expect("mesh address"),
        health: field(&serving, "health").expect("health address"),
        recovered: field(&serving, "recovered_audit_entries").expect("recovered count"),
    }
}

fn probe(identity: &Path, service: &str, addr: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mimir-probe"))
        .args([
            "--identity-dir",
            &identity.display().to_string(),
            "--service",
            service,
            "--addr",
            addr,
            "--purpose",
            "investigation",
            "--prompt",
            "who is assigned to case 42?",
        ])
        .output()
        .expect("mimir-probe runs")
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
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn inference_is_served_over_pqmtls_audited_and_survives_a_restart() {
    let root = scratch("e2e");
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let rogue_idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let server_identity = root.join("server");
    let consumers = root.join("consumers");
    let rogue = root.join("rogue");
    let state = root.join("state");
    issue(
        &idp,
        &server_identity,
        "mimir-server",
        &["inference-server"],
    );
    issue(&idp, &consumers, "forge-worker", &["inference-consumer"]);
    issue(&idp, &consumers, "analyst-tool", &["analyst"]);
    issue(&rogue_idp, &rogue, "forge-worker", &["inference-consumer"]);
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("policy.json"), POLICY).unwrap();

    let server = start(&server_identity, &state);
    assert_eq!(server.recovered, "0");
    let (status, body) = get(&server.health, "/readyz");
    assert_eq!(status, 200, "{body}");

    let served = probe(&consumers, "forge-worker", &server.mesh);
    assert!(served.status.success(), "{}", text(&served.stderr));
    let out = text(&served.stdout);
    assert!(out.contains("locality=InEnclosure"), "{out}");
    assert!(out.contains("audit_seq=0"), "{out}");

    let unpermitted = probe(&consumers, "analyst-tool", &server.mesh);
    assert_eq!(
        unpermitted.status.code(),
        Some(1),
        "a role policy does not permit"
    );
    let forged = probe(&rogue, "forge-worker", &server.mesh);
    assert_eq!(
        forged.status.code(),
        Some(1),
        "another issuer's certificate"
    );

    // The refusals are counted once their connections finish.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, metrics) = get(&server.health, "/metrics");
        assert_eq!(status, 200);
        if metrics.contains("mesh_connections_total{outcome=\"served\"} 1")
            && metrics.contains("mesh_connections_total{outcome=\"refused\"} 2")
        {
            break;
        }
        assert!(Instant::now() < deadline, "{metrics}");
        std::thread::sleep(Duration::from_millis(50));
    }

    let (identity_arg, state_arg) = (
        server_identity.display().to_string(),
        state.display().to_string(),
    );
    let second = Command::new(env!("CARGO_BIN_EXE_mimir-server"))
        .args(server_args(&identity_arg, &state_arg))
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(1));
    assert!(
        text(&second.stderr).contains("in use"),
        "{}",
        text(&second.stderr)
    );

    // A hard kill leaves the lock behind; the restart takes it over and recovers the
    // ledger — only the served call was audited, the refused channels never reached it.
    drop(server);
    let server = start(&server_identity, &state);
    assert_eq!(server.recovered, "1");
    let again = probe(&consumers, "forge-worker", &server.mesh);
    assert!(again.status.success(), "{}", text(&again.stderr));
    assert!(text(&again.stdout).contains("audit_seq=1"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

/// A `llama-server` stand-in on loopback: healthy, streaming `reply` for every
/// completion, and recording each request body it was sent.
fn fake_sidecar(reply: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(value) = lower.strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; length];
            let _ = reader.read_exact(&mut body);
            if request_line.starts_with("GET /health") {
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}");
                continue;
            }
            seen.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&body).into_owned());
            let events = format!(
                "data: {{\"choices\":[{{\"text\":\"{reply}\"}}]}}\n\n\
                 data: {{\"choices\":[{{\"text\":\"\"}}],\"usage\":{{\"prompt_tokens\":9,\"completion_tokens\":4}}}}\n\n\
                 data: [DONE]\n\n"
            );
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{events}",
                events.len()
            );
        }
    });
    (addr, requests)
}

fn strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// A signed Granite model directory, and the trust file for its signing key.
fn signed_model(root: &Path) -> (PathBuf, PathBuf) {
    use model_server::{
        render_manifest_file, sign_manifest, HwProfile, Licence, ModelManifest, WeightFormat,
        WeightsFile, MANIFEST_FILE, VELDRIS_MODEL_SIG_ALG,
    };
    use pqc_agility::backend::fips::FipsSig;
    use pqc_agility::SignatureScheme;

    let dir = root.join("model");
    std::fs::create_dir_all(&dir).unwrap();
    let mut weights = b"GGUF\x03\x00\x00\x00".to_vec();
    weights.extend((0..65_536u32).map(|i| (i % 251) as u8));
    std::fs::write(dir.join("granite-test.gguf"), &weights).unwrap();
    let (vk, sk) = FipsSig::new(VELDRIS_MODEL_SIG_ALG)
        .keypair(&mut OsRng)
        .unwrap();
    let manifest = ModelManifest {
        id: "granite-test".into(),
        family: "Granite".into(),
        version: "1".into(),
        sha256: pqc_agility::hash::sha256(&weights),
        hw: HwProfile::none(),
        licence: Licence {
            name: "Apache-2.0".into(),
            osi_approved: true,
            redistribution_ok: true,
        },
        weights: Some(WeightsFile {
            name: "granite-test.gguf".into(),
            format: WeightFormat::Gguf,
            quantisation: "Q4_K_M".into(),
            size: weights.len() as u64,
        }),
    };
    let text = render_manifest_file(&sign_manifest(manifest, &sk)).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), text).unwrap();
    let hex = vk.as_bytes().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    });
    let trust = root.join("model-signing.pub");
    std::fs::write(
        &trust,
        format!("{} model-signing {hex}\n", VELDRIS_MODEL_SIG_ALG.as_str()),
    )
    .unwrap();
    (dir, trust)
}

#[test]
fn a_signed_model_is_served_through_its_sidecar_and_a_tampered_one_never_starts() {
    let root = scratch("sidecar");
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let server_identity = root.join("server");
    let consumers = root.join("consumers");
    let state = root.join("state");
    issue(
        &idp,
        &server_identity,
        "mimir-server",
        &["inference-server"],
    );
    issue(&idp, &consumers, "forge-worker", &["inference-consumer"]);
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("policy.json"), POLICY).unwrap();

    let (model, trust) = signed_model(&root);
    let (model_arg, trust_arg) = (model.display().to_string(), trust.display().to_string());
    let (sidecar, requests) = fake_sidecar("Olsen holds case 42.");
    let engine = |sidecar: &str| -> Vec<String> {
        [
            "--engine",
            "llama-cpp",
            "--model-dir",
            &model_arg,
            "--model-trust",
            &trust_arg,
            "--sidecar",
            sidecar,
        ]
        .map(String::from)
        .to_vec()
    };

    let args = engine(&sidecar);
    let server = start_with(&server_identity, &state, &strs(&args));
    let (status, body) = get(&server.health, "/readyz");
    assert_eq!(status, 200, "{body}");
    let served = probe(&consumers, "forge-worker", &server.mesh);
    assert!(served.status.success(), "{}", text(&served.stderr));
    let out = text(&served.stdout);
    assert!(out.contains("Olsen holds case 42."), "{out}");
    assert!(out.contains("llama-cpp/granite-test"), "{out}");
    {
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0].contains("<|start_of_role|>user<|end_of_role|>who is assigned to case 42?"),
            "{}",
            requests[0]
        );
    }
    drop(server);

    // With no sidecar answering, the service is not ready and refuses to infer.
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let args = engine(&closed);
    let server = start_with(&server_identity, &state, &strs(&args));
    let (status, _) = get(&server.health, "/readyz");
    assert_ne!(status, 200, "ready without a sidecar");
    let refused = probe(&consumers, "forge-worker", &server.mesh);
    assert_eq!(refused.status.code(), Some(1), "{}", text(&refused.stdout));
    drop(server);

    // One changed weight byte: the service never starts.
    let weights = model.join("granite-test.gguf");
    let mut bytes = std::fs::read(&weights).unwrap();
    bytes[1_000] ^= 1;
    std::fs::write(&weights, &bytes).unwrap();
    let (identity_arg, state_arg) = (
        server_identity.display().to_string(),
        state.display().to_string(),
    );
    let args = engine(&sidecar);
    let tampered = Command::new(env!("CARGO_BIN_EXE_mimir-server"))
        .args(server_args(&identity_arg, &state_arg))
        .args(strs(&args))
        .output()
        .unwrap();
    assert_eq!(tampered.status.code(), Some(1));
    assert!(
        text(&tampered.stderr).contains("do not match the signed digest"),
        "{}",
        text(&tampered.stderr)
    );
    let _ = std::fs::remove_dir_all(&root);
}
