//! RC-17: real on-host fleet agents check in over PQ-mTLS. Two host agents run a
//! genuine hybrid X25519+ML-KEM-768 handshake to the fleet listener and report
//! their installed version + health; the fleet reflects live versions and flags a
//! node that goes silent as stale, and an expected-but-never-seen node as absent.
//! [FR-B-3] [RC-17]
//!
//! (Loopback TCP works under `docker run --network none`; the FIPS backend comes
//! from the dev-dependencies.)

use std::collections::BTreeSet;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bifrost_fleet::{AgentReport, Fleet, NodeStatus};
use bifrost_rollout::VersionId;
use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicySet, Purpose, Resource,
    ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{
    canonical_identity, read_frame, run_initiator, run_responder, write_frame, IdentityCert,
    Initiator, Posture, Responder, Role as RecordRole, ServiceId, TrustAnchor, WorkloadIdentity,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{CryptoProvider, OsRng, Rng, SigId, SigningKey, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;
const CHECKIN_AAD: &[u8] = b"fleet-checkin";
const ACK_AAD: &[u8] = b"fleet-ack";

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let b = z.to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&b[..take]);
            i += take;
        }
    }
}

// --- minimal PQ-mTLS identity fixtures -----------------------------------------

fn keypair(p: &CryptoProvider, seed: u64) -> (VerifyingKey, SigningKey) {
    p.sig_for(ALG).unwrap().keypair(&mut SeedRng(seed)).unwrap()
}

struct Ca {
    anchor: TrustAnchor,
    signing_key: SigningKey,
}
fn ca(p: &CryptoProvider, seed: u64) -> Ca {
    let (vk, sk) = keypair(p, seed);
    Ca {
        anchor: TrustAnchor::new(ALG, vk),
        signing_key: sk,
    }
}

struct Party {
    cert: IdentityCert,
    signing_key: SigningKey,
}
fn party(p: &CryptoProvider, id: &str, role: &str, seed: u64, ca: &Ca) -> Party {
    let (vk, sk) = keypair(p, seed);
    let identity = WorkloadIdentity {
        id: ServiceId(id.into()),
        roles: [Role(role.into())].into_iter().collect(),
        sig_alg: ALG,
        verifying_key: vk,
    };
    let ca_signature = p
        .sig_for(ALG)
        .unwrap()
        .sign(&ca.signing_key, &canonical_identity(&identity))
        .unwrap();
    Party {
        cert: IdentityCert {
            identity,
            ca_sig_alg: ALG,
            ca_signature,
        },
        signing_key: sk,
    }
}

fn channel_resource() -> Resource {
    Resource {
        id: ResourceId("fleet".into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}
fn agent_pdp() -> DefaultPdp {
    DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("r".into()),
            role: Role("agent".into()),
            action: Action("OpenChannel".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    )
}
fn posture() -> Posture {
    Posture {
        level: ClassificationLevel::SECRET,
        compartments: BTreeSet::new(),
    }
}

// --- one on-host agent: PQ-mTLS handshake, then a sealed check-in ----------------

fn agent_check_in(
    addr: std::net::SocketAddr,
    agent: &Party,
    anchor: &TrustAnchor,
    report: &AgentReport,
) {
    let provider = fips_provider();
    let stream = TcpStream::connect(addr).unwrap();
    let mut ch = pq_mtls::TcpChannel::new(stream);
    let mut rng = OsRng;
    let init = Initiator::new(
        &provider,
        agent.cert.clone(),
        &agent.signing_key,
        anchor.clone(),
        Purpose("investigation".into()),
        posture(),
        &mut rng,
    );
    let session = run_initiator(init, &mut ch).expect("agent handshake");
    let mut stream = ch.into_stream();
    let (mut seal, mut open) = session.records(RecordRole::Initiator);
    write_frame(
        &mut stream,
        &seal.seal(CHECKIN_AAD, &report.encode()).unwrap(),
    )
    .unwrap();
    let ack = open
        .open(ACK_AAD, &read_frame(&mut stream).unwrap())
        .unwrap();
    assert_eq!(&ack, b"ACK");
}

#[test]
fn agents_check_in_over_pqmtls_and_staleness_is_detected() {
    let provider = fips_provider();
    let ca = ca(&provider, 1);
    let server_party = party(&provider, "fleet-manager", "server", 20, &ca);
    let host_a = party(&provider, "host-a", "agent", 30, &ca);
    let host_b = party(&provider, "host-b", "agent", 40, &ca);

    let fleet = Arc::new(Mutex::new(Fleet::new()));
    fleet
        .lock()
        .unwrap()
        .set_channel("stable", VersionId::new("v2"));

    // The manager stamps each check-in with its own clock (test-controlled).
    let clock = Arc::new(AtomicU64::new(100));
    let shutdown = Arc::new(AtomicBool::new(false));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let anchor = ca.anchor.clone();

    // --- the fleet listener: authenticate each agent, record its check-in ---
    let (sfleet, sclock, sshutdown, sanchor) = (
        fleet.clone(),
        clock.clone(),
        shutdown.clone(),
        anchor.clone(),
    );
    let server = thread::spawn(move || {
        let provider = fips_provider();
        let pdp = agent_pdp();
        loop {
            if sshutdown.load(Ordering::Relaxed) {
                return;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    // On macOS and Windows an accepted socket inherits the listener's
                    // non-blocking mode (on Linux it does not); the handshake blocks.
                    stream.set_nonblocking(false).unwrap();
                    let mut ch = pq_mtls::TcpChannel::new(stream);
                    let mut rng = OsRng;
                    let resp = Responder::new(
                        &provider,
                        server_party.cert.clone(),
                        &server_party.signing_key,
                        sanchor.clone(),
                        &pdp,
                        channel_resource(),
                        &mut rng,
                    );
                    let session = match run_responder(resp, &mut ch) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let mut stream = ch.into_stream();
                    let (mut seal, mut open) = session.records(RecordRole::Responder);
                    let msg = open
                        .open(CHECKIN_AAD, &read_frame(&mut stream).unwrap())
                        .unwrap();
                    let report = AgentReport::decode(&msg).expect("valid report");
                    sfleet
                        .lock()
                        .unwrap()
                        .check_in(report, sclock.load(Ordering::Relaxed));
                    write_frame(&mut stream, &seal.seal(ACK_AAD, b"ACK").unwrap()).unwrap();
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                }
                Err(_) => return,
            }
        }
    });

    // --- round 1 @ t=100: both hosts check in with their live versions ---
    clock.store(100, Ordering::Relaxed);
    agent_check_in(
        addr,
        &host_a,
        &anchor,
        &AgentReport::new("host-a", "stable", VersionId::new("v2"), true),
    );
    agent_check_in(
        addr,
        &host_b,
        &anchor,
        &AgentReport::new("host-b", "stable", VersionId::new("v1"), true),
    );

    {
        let f = fleet.lock().unwrap();
        // Fleet reflects LIVE installed versions reported by the agents.
        assert_eq!(f.checkin("host-a").unwrap().version, VersionId::new("v2"));
        assert_eq!(f.checkin("host-b").unwrap().version, VersionId::new("v1"));
        // Reconciliation over live state: host-b has drifted from stable=v2.
        let drift = f.drift();
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].install_id, "host-b");
        assert_eq!(drift[0].to, VersionId::new("v2"));
        // Both are live at t=100 (ttl=100).
        assert_eq!(f.node_status("host-a", 100, 100), NodeStatus::Live);
        assert_eq!(f.node_status("host-b", 100, 100), NodeStatus::Live);
    }

    // --- round 2 @ t=200: only host-a checks in again; host-b goes silent ---
    clock.store(200, Ordering::Relaxed);
    agent_check_in(
        addr,
        &host_a,
        &anchor,
        &AgentReport::new("host-a", "stable", VersionId::new("v2"), true),
    );

    {
        let f = fleet.lock().unwrap();
        // At t=250 with ttl=100: host-a (last seen 200) is live; host-b (last seen
        // 100) has not checked in within the window → stale. host-c never appeared.
        assert_eq!(f.node_status("host-a", 250, 100), NodeStatus::Live);
        assert_eq!(f.node_status("host-b", 250, 100), NodeStatus::Stale);
        assert_eq!(f.node_status("host-c", 250, 100), NodeStatus::Absent);
        assert_eq!(f.stale_nodes(250, 100), vec!["host-b".to_string()]);
        assert_eq!(
            f.absent_nodes(&["host-a", "host-b", "host-c"], 250, 100),
            vec!["host-b".to_string(), "host-c".to_string()]
        );
    }

    shutdown.store(true, Ordering::Relaxed);
    server.join().unwrap();
}
