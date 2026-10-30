//! `pqmtls-probe` — a **real, networked** PQ-mTLS handshake smoke for the
//! deployment-acceptance tier (RT-40 / RC-40).
//!
//! Two instances on **different hosts** complete a genuine hybrid
//! X25519+ML-KEM-768 handshake over TCP (the production `backend-fips` path) and
//! exchange one authenticated record each over the AES-256-GCM record layer. A
//! zero exit on the initiator side proves cross-host PQ-mTLS works on the real pod
//! network — not loopback, not a mock.
//!
//! ```text
//!   host-A:  pqmtls-probe responder --listen 0.0.0.0:8443
//!   host-B:  pqmtls-probe initiator --connect host-a:8443
//! ```
//!
//! ## Honest boundary
//! Both sides derive an **ephemeral, deterministic** trust anchor + peer certs
//! from a shared `--ca-seed` so they mutually authenticate without a running CA.
//! This deliberately exercises the *transport + PQ crypto + record* path across
//! hosts; it is **not** a test of production PKI or the HSM — those are covered by
//! the `hsm-signing` check in the same suite. Never point this at production
//! identities; it provisions its own throwaway ones.
#![cfg_attr(not(feature = "backend-fips"), allow(unused))]

use std::env;
use std::process::exit;

#[cfg(feature = "backend-fips")]
fn real_main() -> Result<(), String> {
    use std::collections::BTreeSet;
    use std::net::{TcpListener, TcpStream};

    use policy::{
        Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicySet, Purpose, Resource,
        ResourceId, Role, Rule, RuleId, Selector,
    };
    use pq_mtls::{
        certify, read_frame, run_initiator, run_responder, write_frame, CertTerms, IdentityCert,
        Initiator, Posture, Responder, Role as RecordRole, ServiceId, TcpChannel, TrustAnchor,
        WorkloadIdentity,
    };
    use pqc_agility::backend::fips::provider as fips_provider;
    use pqc_agility::{CryptoProvider, KemSuiteId, Rng, SigId, SigningKey, VerifyingKey};

    // Real ML-DSA-65 identities; the same deterministic RNG the FIPS fixtures use,
    // so both hosts derive an identical CA + peer certs from the shared seed.
    const ALG: SigId = SigId::MlDsa65;
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

    fn keypair(p: &CryptoProvider, seed: u64) -> (VerifyingKey, SigningKey) {
        p.sig_for(ALG).unwrap().keypair(&mut SeedRng(seed)).unwrap()
    }

    // Deterministic CA derived from the shared --ca-seed; both hosts agree on it.
    fn ca(p: &CryptoProvider, seed: u64) -> (TrustAnchor, SigningKey) {
        let (vk, sk) = keypair(p, seed);
        (TrustAnchor::new(ALG, vk), sk)
    }

    fn party(
        p: &CryptoProvider,
        id: &str,
        roles: &[&str],
        seed: u64,
        ca_sk: &SigningKey,
    ) -> (IdentityCert, SigningKey) {
        let (vk, sk) = keypair(p, seed);
        let roles: BTreeSet<Role> = roles.iter().map(|r| Role(r.to_string())).collect();
        let identity = WorkloadIdentity {
            id: ServiceId(id.into()),
            roles,
            sig_alg: ALG,
            verifying_key: vk,
        };
        // A throwaway certificate good for an hour either side of now, cleared to
        // SECRET — the probe's own, never a production identity.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let terms = CertTerms::new(seed, now.saturating_sub(3600), now + 3600, posture());
        (certify(p, ALG, ca_sk, &identity, terms).unwrap(), sk)
    }

    fn channel_resource() -> Resource {
        Resource {
            id: ResourceId("channel".into()),
            marking: Marking {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
        }
    }
    fn permit_pdp(role: &str) -> DefaultPdp {
        DefaultPdp::new(
            PolicySet::new(vec![Rule {
                id: RuleId("r".into()),
                role: Role(role.into()),
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

    // ---- args -------------------------------------------------------------
    let mut args = env::args().skip(1);
    let mode = args.next().ok_or(
        "usage: pqmtls-probe <responder|initiator> --listen|--connect <addr> [--ca-seed N]",
    )?;
    let mut addr = String::new();
    let mut ca_seed: u64 = 42;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--listen" | "--connect" => addr = args.next().ok_or("missing address after flag")?,
            "--ca-seed" => {
                ca_seed = args
                    .next()
                    .ok_or("missing value after --ca-seed")?
                    .parse()
                    .map_err(|_| "bad --ca-seed")?
            }
            other => return Err(format!("unknown flag {other}")),
        }
    }
    if addr.is_empty() {
        return Err("address is required (--listen host:port or --connect host:port)".into());
    }

    let provider = fips_provider();
    let (anchor, ca_sk) = ca(&provider, ca_seed);
    // Fixed per-role seeds relative to the shared CA seed: both hosts compute the
    // same certs, so each trusts the other end.
    let (server_cert, server_sk) = party(
        &provider,
        "svc-server",
        &["server"],
        ca_seed ^ 0x5EED_0001,
        &ca_sk,
    );
    let (client_cert, client_sk) = party(
        &provider,
        "svc-client",
        &["analyst"],
        ca_seed ^ 0x5EED_0002,
        &ca_sk,
    );

    match mode.as_str() {
        "responder" => {
            let listener = TcpListener::bind(&addr).map_err(|e| format!("bind {addr}: {e}"))?;
            eprintln!("pqmtls-probe: responder listening on {addr}");
            let (stream, peer) = listener.accept().map_err(|e| format!("accept: {e}"))?;
            eprintln!("pqmtls-probe: accepted {peer}");
            let mut ch = TcpChannel::new(stream);
            let pdp = permit_pdp("analyst");
            let mut rng = SeedRng(0xA11CE);
            let resp = Responder::new(
                &provider,
                server_cert,
                &server_sk,
                anchor,
                &pdp,
                channel_resource(),
                &mut rng,
            );
            let session = run_responder(resp, &mut ch)
                .map_err(|e| format!("responder handshake failed: {e:?}"))?;
            if session.suite() != KemSuiteId::HybridX25519MlKem768 {
                return Err(format!("unexpected suite {:?}", session.suite()));
            }
            let mut stream = ch.into_stream();
            let (mut seal, mut open) = session.records(RecordRole::Responder);
            let req = open
                .open(
                    b"req",
                    &read_frame(&mut stream).map_err(|e| format!("read req: {e:?}"))?,
                )
                .map_err(|e| format!("open req: {e:?}"))?;
            if req != b"PING" {
                return Err("unexpected request payload".into());
            }
            write_frame(
                &mut stream,
                &seal
                    .seal(b"resp", b"PONG")
                    .map_err(|e| format!("seal: {e:?}"))?,
            )
            .map_err(|e| format!("write resp: {e:?}"))?;
            write_frame(
                &mut stream,
                &seal
                    .seal_close(b"resp")
                    .map_err(|e| format!("seal: {e:?}"))?,
            )
            .map_err(|e| format!("write close: {e:?}"))?;
            eprintln!(
                "pqmtls-probe: OK — cross-host handshake ({:?}) + record round-trip with peer {}",
                session.suite(),
                session.peer().id.0
            );
            Ok(())
        }
        "initiator" => {
            let stream = TcpStream::connect(&addr).map_err(|e| format!("connect {addr}: {e}"))?;
            let mut ch = TcpChannel::new(stream);
            let mut rng = SeedRng(0xB0B);
            let init = Initiator::new(
                &provider,
                client_cert,
                &client_sk,
                anchor,
                ServiceId("svc-server".into()),
                Purpose("investigation".into()),
                posture(),
                &mut rng,
            );
            let session = run_initiator(init, &mut ch)
                .map_err(|e| format!("initiator handshake failed: {e:?}"))?;
            if session.suite() != KemSuiteId::HybridX25519MlKem768 {
                return Err(format!("unexpected suite {:?}", session.suite()));
            }
            let mut stream = ch.into_stream();
            let (mut seal, mut open) = session.records(RecordRole::Initiator);
            write_frame(
                &mut stream,
                &seal
                    .seal(b"req", b"PING")
                    .map_err(|e| format!("seal: {e:?}"))?,
            )
            .map_err(|e| format!("write req: {e:?}"))?;
            let resp = open
                .open(
                    b"resp",
                    &read_frame(&mut stream).map_err(|e| format!("read resp: {e:?}"))?,
                )
                .map_err(|e| format!("open resp: {e:?}"))?;
            if resp != b"PONG" {
                return Err("unexpected response payload".into());
            }
            write_frame(
                &mut stream,
                &seal
                    .seal_close(b"req")
                    .map_err(|e| format!("seal: {e:?}"))?,
            )
            .map_err(|e| format!("write close: {e:?}"))?;
            eprintln!(
                "pqmtls-probe: OK — cross-host handshake ({:?}) + record round-trip with peer {}",
                session.suite(),
                session.peer().id.0
            );
            Ok(())
        }
        other => Err(format!("unknown mode {other} (want responder|initiator)")),
    }
}

fn main() {
    #[cfg(feature = "backend-fips")]
    {
        if let Err(e) = real_main() {
            eprintln!("pqmtls-probe: FAIL — {e}");
            exit(1);
        }
    }
    #[cfg(not(feature = "backend-fips"))]
    {
        let _ = env::args();
        eprintln!(
            "pqmtls-probe: built without `backend-fips`; rebuild with --features backend-fips"
        );
        exit(2);
    }
}
