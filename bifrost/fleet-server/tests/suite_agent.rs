//! `bifrost-agent --suite` end to end, against a throwaway bifrost-server, with fake
//! services and a fake runner (never the real suite): adoption refused on a digest
//! mismatch, then adopted; a release rolled out through every ring to converged, each
//! service restarted from its new slot; a canary that never becomes ready rolled back
//! by the orchestrator and swapped back by the agent; a build signed by a signer the
//! agent does not trust refused, nothing changed; the control plane going away and
//! coming back, nothing changed meanwhile; and the rolled-back slot pruned.
//! [FR-B-2] [FR-B-3] [FR-B-4]
#![cfg(feature = "backend-fips")]

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use attest::Keystore;
use bifrost_artefact::{build_and_sign, generate_sbom, SbomComponent, SignedArtefact};
use bifrost_fleet_server::client;
use iam::{Idp, IDP_SIG_ALG};
use policy::Role;
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::{OsRng, SignatureScheme, SigningKey};
use service_runtime::client::Session;
use service_runtime::json::Json;
use service_runtime::ServiceIdentity;

const POLICY: &str = include_str!("../../../deploy/compose/fleet-policy.json");
const CHANNELS: &str = r#"{ "version": 1, "channels": { "stable": "0.1.0" } }"#;
/// The suite's rings, fast: 1 s soak, 8 s to become ready.
const RINGS: &str = r#"{ "version": 1, "soakSecs": 1, "readyTimeoutSecs": 8, "reportTtlSecs": 30,
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
        "bifrost-suite-e2e-{tag}-{}-{uniq}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn issue(idp: &Idp, dir: &Path, service: &str, roles: &[&str]) {
    fs::create_dir_all(dir).unwrap();
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
    fs::write(dir.join(format!("{service}.cert")), encode_cert(&cert)).unwrap();
    fs::write(dir.join("mesh.anchor"), encode_anchor(&idp.trust_anchor())).unwrap();
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_server(identity: &Path, state: &Path, bind: &str) -> Running {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bifrost-server"))
        .args([
            "--identity-dir",
            &identity.display().to_string(),
            "--state-dir",
            &state.display().to_string(),
            "--bind",
            bind,
            "--health-bind",
            "127.0.0.1:0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("bifrost-server starts");
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    loop {
        match lines.next() {
            Some(Ok(line)) if line.contains("\"message\":\"serving\"") => break,
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
    }
    std::thread::spawn(move || lines.for_each(drop));
    Running(child)
}

fn session(clients: &Path, service: &str, addr: &str) -> Session {
    let identity = ServiceIdentity::load(clients, service).unwrap();
    Session::open(&identity, addr, "fleet-management").unwrap()
}

/// A fake service: a shell script that says which build it is, then prints `ready`
/// (unless it is the never-ready build) and runs until stopped (at most ~5 minutes).
fn fake_service(id: &str, version: &str, never_ready: bool) -> Vec<u8> {
    format!(
        "#!/bin/sh\n# fake {id} {version}\necho \"{id} {version}\" > \"$FAKE_RUN/{id}.version\"\n\
         echo starting\nsleep 0.2\n{}\ni=0\n\
         while [ $i -lt 3000 ] && [ ! -e \"$FAKE_RUN/stop-all\" ]; do sleep 0.1; i=$((i+1)); done\n",
        if never_ready { "echo 'never ready'" } else { "echo ready" }
    )
    .into_bytes()
}

fn build(sk: &SigningKey, id: &str, version: &str, never_ready: bool) -> SignedArtefact {
    let sbom = generate_sbom(
        id,
        version,
        "service",
        vec![SbomComponent::new("pqc-agility", "0.1.0")],
    );
    build_and_sign(
        id,
        version,
        "service",
        fake_service(id, version, never_ready),
        sbom,
        sk,
    )
}

/// The fake runner: `status|restart <service>` as the agent calls them, and `stop-all`.
/// A service runs from `<suite>/active/<service>`; restart waits up to 3 s for `ready`.
fn fake_runner(dir: &Path, suite: &Path, run: &Path) -> PathBuf {
    let path = dir.join("runner.sh");
    fs::write(
        &path,
        format!(
            r#"#!/bin/sh
SUITE="{suite}"; RUN="{run}"
verb=$1; svc=$2
pidf="$RUN/$svc.pid"; log="$RUN/$svc.log"
alive() {{ [ -f "$pidf" ] && kill -0 "$(cat "$pidf")" 2>/dev/null; }}
case $verb in
  status)
    if ! alive; then echo stopped; elif grep -qx ready "$log"; then echo ready; else echo unready; fi ;;
  restart)
    if alive; then kill "$(cat "$pidf")"; while alive; do sleep 0.05; done; fi
    FAKE_RUN="$RUN" nohup "$SUITE/active/$svc" > "$log" 2>&1 &
    pid=$!
    echo $pid > "$pidf"
    echo "$svc $(readlink "$SUITE/active/$svc")" >> "$RUN/restarts.log"
    i=0
    while [ $i -lt 30 ]; do
      grep -qx ready "$log" && {{ echo "$svc ready"; exit 0; }}
      kill -0 $pid 2>/dev/null || {{ echo "$svc exited"; exit 1; }}
      sleep 0.1; i=$((i+1))
    done
    echo "$svc not ready after 3 s"; exit 1 ;;
  stop-all)
    touch "$RUN/stop-all"
    for p in "$RUN"/*.pid; do [ -f "$p" ] && kill "$(cat "$p")" 2>/dev/null; done; exit 0 ;;
  *) echo "usage: status|restart <service> | stop-all"; exit 2 ;;
esac
"#,
            suite = suite.display(),
            run = run.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

struct Fakes(PathBuf);

impl Drop for Fakes {
    fn drop(&mut self) {
        let _ = Command::new(&self.0).arg("stop-all").status();
    }
}

struct Agent {
    child: Running,
    log: PathBuf,
}

impl Agent {
    fn log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

fn agent_command(
    clients: &Path,
    addr: &str,
    suite: &Path,
    runner: &Path,
    trust: &Path,
    adopt_from: &Path,
    initial: Option<&str>,
) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_bifrost-agent"));
    c.args(["--suite", "--identity-dir"])
        .arg(clients)
        .args(["--addr", addr, "--service", "mac-suite"])
        .args(["--install-id", "mac-suite", "--suite-root"])
        .arg(suite)
        .arg("--runner")
        .arg(runner)
        .arg("--trust-dir")
        .arg(trust)
        .arg("--adopt-from")
        .arg(adopt_from)
        .args(["--interval", "1"]);
    if let Some(v) = initial {
        c.args(["--initial-version", v]);
    }
    c
}

fn active(suite: &Path, service: &str) -> String {
    let target = fs::read_link(suite.join("active").join(service)).unwrap();
    target
        .iter()
        .nth(2)
        .and_then(|v| v.to_str())
        .unwrap()
        .to_string()
}

fn running_version(run: &Path, service: &str) -> String {
    fs::read_to_string(run.join(format!("{service}.version")))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn wait_for(what: &str, limit: Duration, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + limit;
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Poll `run` until it is no longer running.
fn finish(rm: &mut Session, run: &str, limit: Duration) -> Json {
    let until = Instant::now() + limit;
    loop {
        let got = client::get_rollout(rm, run).unwrap();
        if got.str_field("state") != Some("running") {
            return got;
        }
        assert!(Instant::now() < until, "{}", got.encode());
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[test]
fn the_suite_agent_adopts_rolls_out_rolls_back_and_refuses_untrusted_builds() {
    let root = scratch("agent");
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let server_identity = root.join("server");
    let clients = root.join("clients");
    let state = root.join("state");
    issue(&idp, &server_identity, "bifrost-server", &["fleet-manager"]);
    issue(&idp, &clients, "release-manager", &["release-manager"]);
    issue(&idp, &clients, "mac-suite", &["fleet-agent"]);
    fs::create_dir_all(state.join("trust/release-signers")).unwrap();
    fs::write(state.join("policy.json"), POLICY).unwrap();
    fs::write(state.join("channels.json"), CHANNELS).unwrap();
    fs::write(state.join("rings.json"), RINGS).unwrap();

    // Two release signers the server trusts; the agent trusts only the first.
    let scheme = FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG);
    let (vk1, sk1) = scheme.keypair(&mut OsRng).unwrap();
    let (vk2, sk2) = scheme.keypair(&mut OsRng).unwrap();
    let line = |id: &str, vk: &pqc_agility::VerifyingKey| {
        format!("ML-DSA-65 {id} {}\n", attest::hex::hex(vk.as_bytes()))
    };
    fs::write(
        state.join("trust/release-signers/release.pub"),
        line("release-1", &vk1) + &line("release-2", &vk2),
    )
    .unwrap();
    let agent_trust = root.join("agent-trust");
    fs::create_dir_all(&agent_trust).unwrap();
    fs::write(agent_trust.join("release.pub"), line("release-1", &vk1)).unwrap();

    // A fixed address, so the server can be restarted where the agent looks for it.
    let addr = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().to_string()
    };
    let server = start_server(&server_identity, &state, &addr);
    let mut rm = session(&clients, "release-manager", &addr);

    // Builds: every service at 0.1.0 and 0.2.0; a forge-worker 0.3.0 that never becomes
    // ready; a forge-worker 0.4.0 signed by the signer the agent does not trust.
    for version in ["0.1.0", "0.2.0"] {
        for id in SERVICES {
            client::register(&mut rm, &build(&sk1, id, version, false), "release-1").unwrap();
        }
    }
    client::register(
        &mut rm,
        &build(&sk1, "forge-worker", "0.3.0", true),
        "release-1",
    )
    .unwrap();
    client::register(
        &mut rm,
        &build(&sk2, "forge-worker", "0.4.0", false),
        "release-2",
    )
    .unwrap();
    client::create_release(&mut rm, "0.1.0", None).unwrap();
    client::create_release(&mut rm, "0.2.0", None).unwrap();
    let on = |forge: &str| -> Vec<(String, String)> {
        SERVICES
            .iter()
            .map(|s| {
                let v = if *s == "forge-worker" { forge } else { "0.2.0" };
                (s.to_string(), v.to_string())
            })
            .collect()
    };
    client::create_release(&mut rm, "0.3.0", Some(&on("0.3.0"))).unwrap();
    client::create_release(&mut rm, "0.4.0", Some(&on("0.4.0"))).unwrap();

    // The installed binaries (dist/mac/bin's stand-in) are the 0.1.0 builds, except that
    // forge-worker and mimir-server were rebuilt.
    let dist = root.join("dist");
    fs::create_dir_all(&dist).unwrap();
    for id in SERVICES {
        fs::write(dist.join(id), fake_service(id, "0.1.0", false)).unwrap();
        fs::set_permissions(dist.join(id), fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(dist.join("forge-worker"), b"#!/bin/sh\n# rebuilt\n").unwrap();
    fs::write(dist.join("mimir-server"), b"#!/bin/sh\n# rebuilt too\n").unwrap();

    let suite = root.join("suite");
    let run = root.join("run");
    fs::create_dir_all(&run).unwrap();
    let runner = fake_runner(&root, &suite, &run);
    let _fakes = Fakes(runner.clone());

    // No --initial-version on a first run: refused.
    let out = agent_command(&clients, &addr, &suite, &runner, &agent_trust, &dist, None)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("pass --initial-version"), "{err}");

    // Adoption with a digest mismatch: refused, every mismatch named, nothing adopted.
    let out = agent_command(
        &clients,
        &addr,
        &suite,
        &runner,
        &agent_trust,
        &dist,
        Some("0.1.0"),
    )
    .output()
    .unwrap();
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("refusing to adopt"), "{err}");
    assert!(err.contains("forge-worker: "), "{err}");
    assert!(err.contains("mimir-server: "), "{err}");
    assert!(!err.contains("http-gateway: "), "{err}");
    assert!(fs::read_dir(suite.join("active")).unwrap().next().is_none());

    // The right binaries: adopted as 0.1.0.
    for id in ["forge-worker", "mimir-server"] {
        fs::write(dist.join(id), fake_service(id, "0.1.0", false)).unwrap();
    }
    let log = root.join("agent.log");
    let agent = Agent {
        child: Running(
            agent_command(
                &clients,
                &addr,
                &suite,
                &runner,
                &agent_trust,
                &dist,
                Some("0.1.0"),
            )
            .stdout(File::create(&log).unwrap())
            .stderr(File::options().append(true).open(&log).unwrap())
            .spawn()
            .unwrap(),
        ),
        log,
    };
    wait_for("adoption", Duration::from_secs(20), || {
        agent.log().contains("adopted")
    });
    for id in SERVICES {
        assert_eq!(active(&suite, id), "0.1.0");
    }
    // The suite starts (as run-native.sh start would), from the active links.
    for id in SERVICES {
        let ok = Command::new(&runner)
            .args(["restart", id])
            .output()
            .unwrap();
        assert!(ok.status.success(), "{id}");
        assert_eq!(running_version(&run, id), format!("{id} 0.1.0"));
    }
    fs::write(run.join("restarts.log"), "").unwrap();

    // A release through every ring, gateway last, to converged.
    let promoted = client::promote(&mut rm, "stable", "0.2.0").unwrap();
    let id = promoted.str_field("id").unwrap().to_string();
    let done = finish(&mut rm, &id, Duration::from_secs(90));
    assert_eq!(
        done.str_field("state"),
        Some("completed"),
        "{}\n{}",
        done.encode(),
        agent.log()
    );
    for id in SERVICES {
        assert_eq!(active(&suite, id), "0.2.0");
        assert_eq!(running_version(&run, id), format!("{id} 0.2.0"));
    }
    let restarts = fs::read_to_string(run.join("restarts.log")).unwrap();
    let order: Vec<&str> = restarts
        .lines()
        .map(|l| l.split(' ').next().unwrap())
        .collect();
    assert_eq!(order.len(), 6, "{restarts}");
    assert_eq!(order[0], "forge-worker", "{restarts}");
    assert_eq!(order[4], "mimir-server", "{restarts}");
    assert_eq!(order[5], "http-gateway", "{restarts}");
    assert!(
        restarts.contains("../releases/0.2.0/http-gateway"),
        "{restarts}"
    );
    // The previous release's slots are kept.
    assert!(suite.join("releases/0.1.0/forge-worker").is_file());

    // A canary that never becomes ready: the orchestrator rolls back, the agent swaps
    // forge-worker back to 0.2.0 from its kept slot.
    let promoted = client::promote(&mut rm, "stable", "0.3.0").unwrap();
    let id = promoted.str_field("id").unwrap().to_string();
    let failed = finish(&mut rm, &id, Duration::from_secs(60));
    assert_eq!(
        failed.str_field("state"),
        Some("rolled-back"),
        "{}",
        failed.encode()
    );
    let reason = failed.str_field("reason").unwrap();
    assert!(
        reason.contains("ring canary: not ready within 8 s"),
        "{reason}"
    );
    assert!(agent.log().contains("restart forge-worker failed"));
    wait_for(
        "forge-worker back on 0.2.0",
        Duration::from_secs(30),
        || {
            active(&suite, "forge-worker") == "0.2.0"
                && running_version(&run, "forge-worker") == "forge-worker 0.2.0"
        },
    );
    // Once settled, the rolled-back build's slot is pruned; the previous release's stays.
    wait_for("the 0.3.0 slot pruned", Duration::from_secs(20), || {
        !suite.join("releases/0.3.0").exists()
    });
    assert!(suite.join("releases/0.1.0/forge-worker").is_file());
    assert!(suite.join("releases/0.2.0/forge-worker").is_file());

    // A build signed by a signer the agent does not anchor: refused, nothing changes,
    // and the canary times out and rolls back.
    let promoted = client::promote(&mut rm, "stable", "0.4.0").unwrap();
    let id = promoted.str_field("id").unwrap().to_string();
    let failed = finish(&mut rm, &id, Duration::from_secs(60));
    assert_eq!(failed.str_field("state"), Some("rolled-back"));
    let log = agent.log();
    assert!(
        log.contains("refused forge-worker@0.4.0") && log.contains("is not one this caller trusts"),
        "{log}"
    );
    assert_eq!(active(&suite, "forge-worker"), "0.2.0");
    assert_eq!(running_version(&run, "forge-worker"), "forge-worker 0.2.0");
    assert!(!suite.join("releases/0.4.0").exists());

    // The control plane goes away: the agent keeps running and changes nothing; when it
    // comes back, the agent checks in again.
    drop(server);
    wait_for("a failed check-in", Duration::from_secs(20), || {
        agent.log().contains("check-in failed")
    });
    std::thread::sleep(Duration::from_secs(3));
    let mut child = agent.child;
    assert!(child.0.try_wait().unwrap().is_none(), "the agent exited");
    for id in SERVICES {
        assert_eq!(active(&suite, id), "0.2.0");
    }
    let _server = start_server(&server_identity, &state, &addr);
    wait_for("the agent to reconnect", Duration::from_secs(60), || {
        fs::read_to_string(root.join("agent.log"))
            .unwrap_or_default()
            .contains("control plane reachable again")
    });
    assert!(child.0.try_wait().unwrap().is_none(), "the agent exited");
    assert!(!suite.join("swap.journal").exists());

    drop(child);
    drop(_fakes);
    let _ = fs::remove_dir_all(&root);
}
