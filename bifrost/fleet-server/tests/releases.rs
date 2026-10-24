//! Releases, channels and rollouts in `bifrost-server`, end to end as a real process on
//! real sockets: only a release manager creates, promotes and rolls back, operators read,
//! agents only check in; a release is refused unless every artefact in it is held and
//! verified; an agent following its desired versions takes a release through every ring;
//! a ring that does not become ready rolls back with the reason; one run per channel; a
//! manual rollback; a restart mid-ring resumes the run; and the old binary CheckIn still
//! works beside the per-service one. [FR-B-2] [FR-B-3] [NFR-SEC-1] [RC-12]
#![cfg(feature = "backend-fips")]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use attest::Keystore;
use bifrost_artefact::{build_and_sign, generate_sbom, SbomComponent, SignedArtefact};
use bifrost_fleet_server::client::{self, CallError, ServiceReport};
use bifrost_fleet_server::outcome;
use iam::{Idp, IDP_SIG_ALG};
use policy::Role;
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::{OsRng, SignatureScheme, SigningKey};
use service_runtime::client::Session;
use service_runtime::json::Json;
use service_runtime::ServiceIdentity;

/// The shipped policy: the test checks the file deployments use.
const POLICY: &str = include_str!("../../../deploy/compose/fleet-policy.json");
const CHANNELS: &str = r#"{ "version": 1, "channels": { "stable": "0.1.0" } }"#;
/// The suite's rings, fast: 1 s soak, 4 s to become ready.
const RINGS: &str = r#"{ "version": 1, "soakSecs": 1, "readyTimeoutSecs": 4, "reportTtlSecs": 30,
  "rings": [
    { "name": "canary", "services": ["forge-worker"] },
    { "name": "ring-2", "services": ["midgard-replay", "heimdall-feed", "heimdall-source"] },
    { "name": "ring-3", "services": ["mimir-server"] },
    { "name": "last", "services": ["http-gateway"] } ] }"#;
const SERVICES: [&str; 6] = [
    "forge-worker",
    "midgard-replay",
    "heimdall-feed",
    "heimdall-source",
    "mimir-server",
    "http-gateway",
];

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "bifrost-releases-e2e-{tag}-{}-{uniq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

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

fn start(identity: &Path, state: &Path) -> Running {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bifrost-server"))
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
        .expect("bifrost-server starts");
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
                panic!("bifrost-server did not start: {stderr}");
            }
        }
    };
    std::thread::spawn(move || lines.for_each(drop));
    Running {
        child,
        mesh: field(&serving, "mesh").expect("mesh address"),
        serving,
    }
}

fn session(clients: &Path, service: &str, addr: &str) -> Session {
    let identity = ServiceIdentity::load(clients, service).unwrap();
    Session::open(&identity, addr, "fleet-management").unwrap()
}

fn build(sk: &SigningKey, id: &str, version: &str) -> SignedArtefact {
    let sbom = generate_sbom(
        id,
        version,
        "service",
        vec![SbomComponent::new("pqc-agility", "0.1.0")],
    );
    let payload = format!("{id} {version} build").repeat(100).into_bytes();
    build_and_sign(id, version, "service", payload, sbom, sk)
}

fn code<T: std::fmt::Debug>(result: Result<T, CallError>) -> (u8, String) {
    match result {
        Err(CallError {
            code: Some(c),
            message,
        }) => (c, message),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn denied<T: std::fmt::Debug>(result: Result<T, CallError>, action: &str) {
    let (c, message) = code(result);
    assert_eq!(c, outcome::DENIED, "{message}");
    assert!(message.contains(action), "{message}");
}

/// An agent that installs whatever it is told, at once, and reports it healthy.
struct Agent {
    session: Session,
    versions: Vec<(String, String)>,
    /// Services that report unhealthy whatever they run.
    broken: Vec<&'static str>,
    /// When set, the agent ignores its desired versions.
    frozen: bool,
}

impl Agent {
    fn check_in(&mut self) -> client::DesiredState {
        let reports: Vec<ServiceReport> = self
            .versions
            .iter()
            .map(|(name, version)| ServiceReport {
                name: name.clone(),
                version: version.clone(),
                healthy: !self.broken.contains(&name.as_str()),
            })
            .collect();
        let desired = client::check_in_services(&mut self.session, "mac-suite", "stable", &reports)
            .expect("the agent checks in");
        if !self.frozen {
            self.versions = desired.desired.clone();
        }
        desired
    }
}

fn run_state(run: &Json) -> (String, Vec<String>) {
    let phases = match run.get("rings") {
        Some(Json::Arr(rings)) => rings
            .iter()
            .map(|r| r.str_field("phase").unwrap().to_string())
            .collect(),
        _ => panic!("no rings in {}", run.encode()),
    };
    (run.str_field("state").unwrap().to_string(), phases)
}

/// Check in once a second until `run` is no longer running, or `limit` passes.
fn drive(agent: &mut Agent, rm: &mut Session, run: &str, limit: Duration) -> Json {
    let until = Instant::now() + limit;
    loop {
        agent.check_in();
        let got = client::get_rollout(rm, run).unwrap();
        if got.str_field("state") != Some("running") || Instant::now() > until {
            return got;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[test]
fn releases_roll_out_ring_by_ring_roll_back_and_resume_after_a_restart() {
    let root = scratch("e2e");
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let server_identity = root.join("server");
    let clients = root.join("clients");
    let state = root.join("state");
    issue(&idp, &server_identity, "bifrost-server", &["fleet-manager"]);
    issue(&idp, &clients, "release-manager", &["release-manager"]);
    issue(&idp, &clients, "fleet-operator", &["fleet-operator"]);
    issue(&idp, &clients, "mac-suite", &["fleet-agent"]);
    issue(&idp, &clients, "other-host", &["fleet-agent"]);
    std::fs::create_dir_all(state.join("trust/release-signers")).unwrap();
    std::fs::write(state.join("policy.json"), POLICY).unwrap();
    std::fs::write(state.join("channels.json"), CHANNELS).unwrap();
    std::fs::write(state.join("rings.json"), RINGS).unwrap();
    let (vk, sk) = FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
        .keypair(&mut OsRng)
        .unwrap();
    std::fs::write(
        state.join("trust/release-signers/release.pub"),
        format!("ML-DSA-65 release-1 {}\n", attest::hex::hex(vk.as_bytes())),
    )
    .unwrap();

    let server = start(&server_identity, &state);
    let mut rm = session(&clients, "release-manager", &server.mesh);
    let mut ops = session(&clients, "fleet-operator", &server.mesh);
    let mut agent_session = session(&clients, "mac-suite", &server.mesh);

    // Six verified builds at 0.1.0 and 0.2.0; at 0.3.0 only the canary, and a tampered
    // forge-worker 0.4.0 that the gate rejects.
    for version in ["0.1.0", "0.2.0"] {
        for id in SERVICES {
            client::register(&mut rm, &build(&sk, id, version), "release-1").unwrap();
        }
    }
    client::register(&mut rm, &build(&sk, "forge-worker", "0.3.0"), "release-1").unwrap();
    let mut tampered = build(&sk, "forge-worker", "0.4.0");
    tampered.payload[10] ^= 1;
    assert!(client::register(&mut rm, &tampered, "release-1").is_err());

    // Policy: operators and agents may not change releases; agents may not read them.
    denied(
        client::create_release(&mut ops, "0.1.0", None),
        "CreateRelease",
    );
    denied(client::promote(&mut ops, "stable", "0.2.0"), "Promote");
    denied(client::rollback(&mut ops, "stable", "x", None), "Rollback");
    denied(client::list_releases(&mut agent_session), "ListReleases");
    denied(
        client::list_rollouts(&mut agent_session, None),
        "ListRollouts",
    );
    denied(
        client::get_rollout(&mut agent_session, "run-1"),
        "GetRollout",
    );
    denied(
        client::check_in_services(&mut rm, "release-manager", "stable", &[]),
        "CheckIn",
    );

    // Only verified artefacts make a release.
    let (c, m) = code(client::create_release(&mut rm, "0.3.0", None));
    assert_eq!(c, outcome::NOT_FOUND, "{m}");
    assert!(
        m.contains("http-gateway@0.3.0 is not in the registry"),
        "{m}"
    );
    assert!(!m.contains("forge-worker@0.3.0"), "{m}");
    let mixed: Vec<(String, String)> = SERVICES
        .iter()
        .map(|s| {
            let v = if *s == "forge-worker" {
                "0.4.0"
            } else {
                "0.1.0"
            };
            (s.to_string(), v.to_string())
        })
        .collect();
    let (c, m) = code(client::create_release(&mut rm, "0.4.0", Some(&mixed)));
    assert_eq!(c, outcome::REJECTED, "{m}");
    assert!(
        m.contains("forge-worker@0.4.0 was rejected (digest-mismatch)"),
        "{m}"
    );
    let short: Vec<(String, String)> = mixed[1..].to_vec();
    let (c, m) = code(client::create_release(&mut rm, "0.5.0", Some(&short)));
    assert_eq!(c, outcome::MALFORMED, "{m}");
    assert!(m.contains("no artefact for forge-worker"), "{m}");

    let r1 = client::create_release(&mut rm, "0.1.0", None).unwrap();
    assert_eq!(r1.str_field("createdBy"), Some("release-manager"));
    client::create_release(&mut rm, "0.2.0", None).unwrap();
    // A release may mix artefact versions.
    let mut canary_only = SERVICES
        .iter()
        .map(|s| (s.to_string(), "0.1.0".to_string()))
        .collect::<Vec<_>>();
    canary_only[0].1 = "0.3.0".into();
    let r3 = client::create_release(&mut rm, "0.3.0", Some(&canary_only)).unwrap();
    assert_eq!(
        r3.get("artefacts")
            .and_then(|a| a.str_field("forge-worker")),
        Some("0.3.0")
    );
    let (c, _) = code(client::create_release(&mut rm, "0.2.0", None));
    assert_eq!(c, outcome::CONFLICT);

    let listing = client::list_releases(&mut ops).unwrap();
    match listing.get("releases") {
        Some(Json::Arr(items)) => assert_eq!(items.len(), 3),
        _ => panic!("{}", listing.encode()),
    }
    let channel = match listing.get("channels") {
        Some(Json::Arr(items)) => items[0].clone(),
        _ => panic!("{}", listing.encode()),
    };
    assert_eq!(channel.str_field("current"), Some("0.1.0"));
    assert_eq!(
        channel
            .get("desired")
            .and_then(|d| d.str_field("http-gateway")),
        Some("0.1.0")
    );

    // The agent is on 0.1.0. Promote 0.2.0: the canary is raised first.
    let mut agent = Agent {
        session: agent_session,
        versions: SERVICES
            .iter()
            .map(|s| (s.to_string(), "0.1.0".to_string()))
            .collect(),
        broken: vec![],
        frozen: false,
    };
    // An agent checks in only for itself.
    let mut other = session(&clients, "other-host", &server.mesh);
    let (c, m) = code(client::check_in_services(
        &mut other,
        "mac-suite",
        "stable",
        &[],
    ));
    assert_eq!(c, outcome::DENIED, "{m}");

    let run = client::promote(&mut rm, "stable", "0.2.0").unwrap();
    assert_eq!(run.str_field("id"), Some("run-1"));
    assert_eq!(run_state(&run).1[0], "waiting");
    let (c, m) = code(client::promote(&mut rm, "stable", "0.3.0"));
    assert_eq!(c, outcome::CONFLICT, "{m}");
    assert!(m.contains("one run per channel"), "{m}");
    let desired = agent.check_in();
    assert_eq!(desired.run.as_deref(), Some("run-1"));
    assert_eq!(desired.target.as_deref(), Some("0.2.0"));
    let of = |d: &client::DesiredState, s: &str| {
        d.desired
            .iter()
            .find(|(n, _)| n == s)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    assert_eq!(of(&desired, "forge-worker"), "0.2.0");
    assert_eq!(of(&desired, "http-gateway"), "0.1.0");

    // Through the canary and into ring 2, then a restart mid-run.
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        agent.check_in();
        let got = client::get_rollout(&mut rm, "run-1").unwrap();
        if run_state(&got).1[1] == "soaking" || run_state(&got).1[1] == "passed" {
            break;
        }
        assert!(Instant::now() < until, "{}", got.encode());
        std::thread::sleep(Duration::from_millis(300));
    }
    drop(server);
    let server = start(&server_identity, &state);
    assert_eq!(
        field(&server.serving, "active_rollouts").as_deref(),
        Some("run-1")
    );
    let mut rm = session(&clients, "release-manager", &server.mesh);
    let mut ops = session(&clients, "fleet-operator", &server.mesh);
    agent.session = session(&clients, "mac-suite", &server.mesh);
    let got = client::get_rollout(&mut rm, "run-1").unwrap();
    assert_eq!(got.str_field("state"), Some("running"), "{}", got.encode());
    assert_eq!(run_state(&got).1[0], "passed");
    let done = drive(&mut agent, &mut rm, "run-1", Duration::from_secs(40));
    assert_eq!(
        run_state(&done),
        ("completed".to_string(), vec!["passed".to_string(); 4]),
        "{}",
        done.encode()
    );
    assert!(agent.versions.iter().all(|(_, v)| v == "0.2.0"));
    // The fleet view follows: the channel's desired is 0.2.0 and the agent is on it.
    let fleet = Command::new(env!("CARGO_BIN_EXE_bifrost-agent"))
        .args([
            "--identity-dir",
            &clients.display().to_string(),
            "--service",
            "fleet-operator",
            "--addr",
            &server.mesh,
            "--status",
        ])
        .output()
        .unwrap();
    let fleet = String::from_utf8_lossy(&fleet.stdout).into_owned();
    assert!(fleet.contains("\"desired\":\"0.2.0\""), "{fleet}");
    assert!(fleet.contains("\"converged\":true"), "{fleet}");

    // A bad canary: forge-worker 0.3.0 never becomes healthy. After 4 s the ring fails
    // and every changed service is desired back on 0.2.0, with the reason recorded.
    agent.broken = vec!["forge-worker"];
    let run = client::promote(&mut rm, "stable", "0.3.0").unwrap();
    let id = run.str_field("id").unwrap().to_string();
    let failed = drive(&mut agent, &mut rm, &id, Duration::from_secs(20));
    assert_eq!(
        failed.str_field("state"),
        Some("rolled-back"),
        "{}",
        failed.encode()
    );
    assert_eq!(run_state(&failed).1[0], "failed");
    let reason = failed.str_field("reason").unwrap();
    assert!(
        reason.contains("ring canary: not ready within 4 s"),
        "{reason}"
    );
    assert!(
        reason.contains("forge-worker on mac-suite unhealthy"),
        "{reason}"
    );
    assert_eq!(failed.str_field("endedBy"), Some("bifrost-orchestrator"));
    agent.broken.clear();
    let desired = agent.check_in();
    assert!(desired.desired.iter().all(|(_, v)| v == "0.2.0"));
    assert!(desired.run.is_none());

    // A manual rollback of a run in progress, with its reason.
    agent.frozen = true;
    let run = client::promote(&mut rm, "stable", "0.3.0").unwrap();
    let id = run.str_field("id").unwrap().to_string();
    let (c, _) = code(client::rollback(&mut rm, "stable", "  ", None));
    assert_eq!(c, outcome::MALFORMED);
    let back = client::rollback(&mut rm, "stable", "operator saw errors", None).unwrap();
    assert_eq!(back.str_field("id"), Some(id.as_str()));
    assert_eq!(back.str_field("state"), Some("rolled-back"));
    assert_eq!(
        back.str_field("reason"),
        Some("manual: operator saw errors")
    );
    assert_eq!(back.str_field("endedBy"), Some("release-manager"));

    // With nothing in progress, a rollback moves the channel back to its previous release.
    let back = client::rollback(&mut rm, "stable", "back to the known good", None).unwrap();
    assert_eq!(back.str_field("kind"), Some("rollback"));
    assert_eq!(back.str_field("release"), Some("0.1.0"));
    let listing = client::list_releases(&mut rm).unwrap();
    let channel = match listing.get("channels") {
        Some(Json::Arr(items)) => items[0].clone(),
        _ => panic!(),
    };
    assert_eq!(channel.str_field("current"), Some("0.1.0"));
    assert_eq!(channel.str_field("previous"), Some("0.2.0"));

    let rollouts = client::list_rollouts(&mut ops, Some("stable")).unwrap();
    let ids: Vec<&str> = match rollouts.get("rollouts") {
        Some(Json::Arr(items)) => items.iter().map(|r| r.str_field("id").unwrap()).collect(),
        _ => panic!(),
    };
    assert_eq!(ids, ["run-4", "run-3", "run-2", "run-1"]);
    let (c, _) = code(client::get_rollout(&mut ops, "run-99"));
    assert_eq!(c, outcome::NOT_FOUND);

    // The old binary CheckIn still works beside the per-service one.
    let legacy = Command::new(env!("CARGO_BIN_EXE_bifrost-agent"))
        .args([
            "--identity-dir",
            &clients.display().to_string(),
            "--service",
            "other-host",
            "--addr",
            &server.mesh,
            "--channel",
            "stable",
            "--version",
            "0.1.0",
        ])
        .output()
        .unwrap();
    assert!(
        legacy.status.success(),
        "{}",
        String::from_utf8_lossy(&legacy.stderr)
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}
