//! RC-15: a rolling upgrade driven against a **real** multi-replica fleet — actual
//! TCP servers, live readiness probes, and continuous request traffic through a
//! retrying load balancer during the roll. Asserts zero failed requests and
//! automatic rollback on a version that never becomes ready — observed against real
//! serving replicas, not a counter. [AC-M-7] [NFR-AVA-1]
//!
//! (Loopback TCP works under `docker run --network none` — this is a genuine
//! network fleet, just in-process.)

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use scheduler::{
    OrchestratorError, ReplicaController, ReplicaId, Scheduler, UpgradeResult, VersionId,
};

// --- a real replica: a TCP server that reports its version iff serving ----------

struct Replica {
    addr: SocketAddr,
    version: Mutex<String>,
    serving: AtomicBool,
    shutdown: Arc<AtomicBool>,
}

impl Replica {
    fn spawn(shutdown: Arc<AtomicBool>) -> Arc<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let r = Arc::new(Replica {
            addr,
            version: Mutex::new(String::new()),
            serving: AtomicBool::new(false),
            shutdown,
        });
        let rc = r.clone();
        thread::spawn(move || {
            // Accept loop: always answers (a not-serving replica returns
            // UNAVAILABLE, like a pod removed from a Service's endpoints).
            loop {
                if rc.shutdown.load(Ordering::Relaxed) {
                    return;
                }
                match listener.accept() {
                    Ok((mut s, _)) => {
                        let mut buf = [0u8; 32];
                        let _ = s.read(&mut buf);
                        let body = if rc.serving.load(Ordering::Relaxed) {
                            format!("OK {}\n", rc.version.lock().unwrap())
                        } else {
                            "UNAVAILABLE\n".to_string()
                        };
                        let _ = s.write_all(body.as_bytes());
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => return,
                }
            }
        });
        r
    }
}

/// One request to `addr`. `Some(body)` on a completed response, `None` on a
/// connection error. A response starting `OK` means the replica served it.
fn request(addr: SocketAddr) -> Option<String> {
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(200)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(200))).ok();
    s.write_all(b"work\n").ok()?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

// --- the real replica controller over that fleet --------------------------------

struct LocalFleet {
    replicas: Vec<Arc<Replica>>,
    index: BTreeMap<String, usize>,
    warmup: Duration,
    deadline: Duration,
    /// Versions that never become ready (an injected bad build).
    unhealthy: Vec<String>,
    shutdown: Arc<AtomicBool>,
}

impl LocalFleet {
    fn new(n: usize, unhealthy: &[&str]) -> (Self, Vec<ReplicaId>) {
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut replicas = Vec::new();
        let mut index = BTreeMap::new();
        let mut ids = Vec::new();
        for i in 0..n {
            replicas.push(Replica::spawn(shutdown.clone()));
            let id = format!("r{i}");
            index.insert(id.clone(), i);
            ids.push(ReplicaId(id));
        }
        (
            Self {
                replicas,
                index,
                warmup: Duration::from_millis(20),
                deadline: Duration::from_millis(800),
                unhealthy: unhealthy.iter().map(|s| s.to_string()).collect(),
                shutdown,
            },
            ids,
        )
    }

    fn addr(&self, id: &ReplicaId) -> SocketAddr {
        self.replicas[self.index[&id.0]].addr
    }

    fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl ReplicaController for LocalFleet {
    fn deploy(&self, replica: &ReplicaId, version: &VersionId) -> Result<(), OrchestratorError> {
        let r = self.replicas[self.index[&replica.0]].clone();
        *r.version.lock().unwrap() = version.0.clone();
        r.serving.store(false, Ordering::Relaxed); // out of service while rolling
        let healthy = !self.unhealthy.contains(&version.0);
        let warmup = self.warmup;
        thread::spawn(move || {
            if healthy {
                thread::sleep(warmup); // readiness gate: ready only after warmup
                r.serving.store(true, Ordering::Relaxed);
            }
            // an unhealthy version never flips serving → its probe never passes
        });
        Ok(())
    }

    fn await_ready(&self, replica: &ReplicaId) -> bool {
        let addr = self.addr(replica);
        let start = std::time::Instant::now();
        while start.elapsed() < self.deadline {
            if request(addr).map(|b| b.starts_with("OK")).unwrap_or(false) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn is_ready(&self, replica: &ReplicaId) -> bool {
        request(self.addr(replica))
            .map(|b| b.starts_with("OK"))
            .unwrap_or(false)
    }
}

/// A retrying load balancer: for each request it tries replicas round-robin and
/// succeeds on the first that serves (a real client routing around a rolling pod).
/// Counts a failure only if NO replica served — i.e. real downtime.
fn run_load(
    addrs: Vec<SocketAddr>,
    running: Arc<AtomicBool>,
    ok: Arc<AtomicU64>,
    fail: Arc<AtomicU64>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let n = addrs.len();
        let mut start = 0usize;
        while running.load(Ordering::Relaxed) {
            let mut served = false;
            for k in 0..n {
                let a = addrs[(start + k) % n];
                if let Some(b) = request(a) {
                    if b.starts_with("OK") {
                        served = true;
                        break;
                    }
                }
            }
            start = start.wrapping_add(1);
            if served {
                ok.fetch_add(1, Ordering::Relaxed);
            } else {
                fail.fetch_add(1, Ordering::Relaxed);
            }
            thread::sleep(Duration::from_millis(1));
        }
    })
}

fn boot(fleet: &LocalFleet, ids: &[ReplicaId], v: &VersionId) {
    for id in ids {
        fleet.deploy(id, v).unwrap();
    }
    for id in ids {
        assert!(fleet.await_ready(id), "replica {} failed to boot", id.0);
    }
}

#[test]
fn rolling_upgrade_serves_every_request_with_real_replicas() {
    let (fleet, ids) = LocalFleet::new(3, &[]);
    let (v1, v2) = (VersionId("v1".into()), VersionId("v2".into()));
    boot(&fleet, &ids, &v1);

    let addrs: Vec<SocketAddr> = ids.iter().map(|id| fleet.addr(id)).collect();
    let running = Arc::new(AtomicBool::new(true));
    let (ok, fail) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let lg = run_load(addrs, running.clone(), ok.clone(), fail.clone());
    thread::sleep(Duration::from_millis(30)); // let traffic ramp

    // min_available = 2 of 3: one replica may be out of service at a time.
    let report = Scheduler::new(2)
        .drive_rolling_upgrade(&ids, &v1, &v2, &fleet)
        .unwrap();

    running.store(false, Ordering::Relaxed);
    lg.join().unwrap();

    assert_eq!(report.result, UpgradeResult::Upgraded);
    assert_eq!(report.final_version, v2);
    assert_eq!(
        report.min_available_observed, 2,
        "availability, measured by live probes, never dropped below min_available"
    );
    assert!(report.all_healthy);
    assert_eq!(
        fail.load(Ordering::Relaxed),
        0,
        "zero requests failed during the rollout"
    );
    assert!(
        ok.load(Ordering::Relaxed) > 10,
        "the load generator actually drove traffic"
    );
    // Every replica really is serving v2 now.
    for id in &ids {
        assert_eq!(request(fleet.addr(id)).unwrap().trim(), "OK v2");
    }
    fleet.stop();
}

#[test]
fn unhealthy_new_version_auto_rolls_back_with_zero_failures() {
    // v2 is a bad build: it never passes its readiness probe.
    let (fleet, ids) = LocalFleet::new(3, &["v2"]);
    let (v1, v2) = (VersionId("v1".into()), VersionId("v2".into()));
    boot(&fleet, &ids, &v1);

    let addrs: Vec<SocketAddr> = ids.iter().map(|id| fleet.addr(id)).collect();
    let running = Arc::new(AtomicBool::new(true));
    let (ok, fail) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let lg = run_load(addrs, running.clone(), ok.clone(), fail.clone());
    thread::sleep(Duration::from_millis(30));

    let report = Scheduler::new(2)
        .drive_rolling_upgrade(&ids, &v1, &v2, &fleet)
        .unwrap();

    running.store(false, Ordering::Relaxed);
    lg.join().unwrap();

    // The bad version never came ready → automatic rollback to v1.
    assert_eq!(report.result, UpgradeResult::RolledBack);
    assert_eq!(report.final_version, v1);
    assert!(
        report.min_available_observed >= 2,
        "availability stayed at/above min_available even during the failed roll"
    );
    assert_eq!(
        fail.load(Ordering::Relaxed),
        0,
        "zero requests failed during the rolled-back upgrade"
    );
    // Every replica is back on the known-good v1 and serving.
    for id in &ids {
        assert_eq!(request(fleet.addr(id)).unwrap().trim(), "OK v1");
    }
    fleet.stop();
}
