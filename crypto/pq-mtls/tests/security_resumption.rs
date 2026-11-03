//! Resumption runs a fresh KEM exchange, its binder covers the KEM share, and the
//! resumed traffic secret uses the RFC 5869 roles (public salt, secret input keying
//! material). Proved over real sockets on the production FIPS backend, with an
//! on-path relay that can rewrite either resumption message. [NFR-SEC-1] [RC-25]
//! [VLD-ANB-MIDGARD-0046]
#![cfg(feature = "backend-fips")]

mod fips_fixtures;
use fips_fixtures::{ca, channel_resource, party, permit_pdp, posture, provider, Ca, SeedRng};

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};

use policy::Purpose;
use pq_mtls::{
    issue_ticket, mutual_handshake, read_frame, run_resume_initiator, run_resume_responder,
    write_frame, ClientTicket, Initiator, MtlsError, Responder, Resumed, Session, TicketKey,
    MODE_RESUME,
};
use pqc_agility::hash::{hkdf_sha256_32, sha256};
use pqc_agility::{Ciphertext, KemPublicKey, KemSuiteId, Rng};

const NOW: u64 = 1_800_000_000;
const RESUME_INFO: &[u8] = b"midgard-pqmtls-resume-traffic-v3";

/// A completed full handshake, a ticket issued on it, and the CA.
struct Fixture {
    ca: Ca,
    client: Session,
    key: TicketKey,
    ticket: ClientTicket,
}

fn fixture() -> Fixture {
    let provider = provider();
    let ca = ca(&provider, 500);
    let client = party(&provider, "svc-client", &["analyst"], 510, &ca);
    let server = party(&provider, "svc-server", &["server"], 520, &ca);
    let pdp = permit_pdp("analyst");
    let mut crng = SeedRng(1);
    let mut srng = SeedRng(2);
    let mut init = Initiator::new(
        &provider,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        server.cert.identity.id.clone(),
        Purpose("investigation".into()),
        posture(),
        &mut crng,
    );
    let mut resp = Responder::new(
        &provider,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        channel_resource(),
        &mut srng,
    );
    let (cs, ss) = mutual_handshake(&mut init, &mut resp).unwrap();
    let key = TicketKey::generate(&mut SeedRng(3));
    let bytes = issue_ticket(&key, &ss, NOW, 3_600, &mut SeedRng(4)).unwrap();
    let ticket = ClientTicket::new(&cs, bytes);
    Fixture {
        ca,
        client: cs,
        key,
        ticket,
    }
}

// ---- message layout (as run_resume_initiator / run_resume_responder send it) --

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

/// `u32 len || ticket || nonce_c(32) || suite || u32 len || kem_pk || binder(32)`:
/// the range of the KEM public key bytes and its suite.
fn hello_kem_pk(hello: &[u8]) -> (KemSuiteId, std::ops::Range<usize>) {
    let suite_at = 4 + u32_at(hello, 0) + 32;
    let suite = KemSuiteId::from_u8(hello[suite_at]).unwrap();
    let len = u32_at(hello, suite_at + 1);
    let start = suite_at + 5;
    (suite, start..start + len)
}

/// `nonce_s(32) || suite || u32 len || ct || finished_r(32)`: the range of the
/// ciphertext bytes.
fn accept_ct(accept: &[u8]) -> std::ops::Range<usize> {
    let len = u32_at(accept, 33);
    37..37 + len
}

type Rewrite<'a> = &'a (dyn Fn(&[u8], Vec<u8>) -> Vec<u8> + Sync);

/// What one relayed resumption produced.
struct Run {
    client: Result<Session, MtlsError>,
    server: Result<Resumed, MtlsError>,
    /// The ResumeHello and ResumeAccept as the client sent and received them.
    hello: Vec<u8>,
    accept: Vec<u8>,
    /// How many times the responder consulted its replay guard.
    replay_checks: usize,
}

/// Resume through an on-path relay. `on_hello(&[], hello)` rewrites the client's
/// ResumeHello; `on_accept(hello, accept)` the server's ResumeAccept.
fn resume_via(f: &Fixture, client_seed: u64, on_hello: Rewrite, on_accept: Rewrite) -> Run {
    let server_l = TcpListener::bind("127.0.0.1:0").unwrap();
    let server_addr = server_l.local_addr().unwrap();
    let relay_l = TcpListener::bind("127.0.0.1:0").unwrap();
    let relay_addr = relay_l.local_addr().unwrap();
    let provider = provider();
    let pdp = permit_pdp("analyst");
    let resource = channel_resource();
    let replay_checks = AtomicUsize::new(0);
    std::thread::scope(|s| {
        let server = s.spawn(|| {
            let (mut sock, _) = server_l.accept().unwrap();
            let mut mode = [0u8; 1];
            sock.read_exact(&mut mode).unwrap();
            assert_eq!(mode[0], MODE_RESUME);
            run_resume_responder(
                &mut sock,
                &[&f.key],
                &f.ca.anchor,
                &provider,
                &pdp,
                &resource,
                NOW + 10,
                |_t, _expires_at| {
                    replay_checks.fetch_add(1, Ordering::SeqCst);
                    true
                },
                &mut SeedRng(2),
            )
        });
        let relay = s.spawn(|| {
            let (mut from_client, _) = relay_l.accept().unwrap();
            let mut to_server = TcpStream::connect(server_addr).unwrap();
            let mut mode = [0u8; 1];
            from_client.read_exact(&mut mode).unwrap();
            to_server.write_all(&mode).unwrap();
            let hello = read_frame(&mut from_client).unwrap();
            write_frame(&mut to_server, &on_hello(&[], hello.clone())).unwrap();
            let accept = read_frame(&mut to_server).ok();
            let delivered = accept.map(|a| on_accept(&hello, a));
            if let Some(a) = &delivered {
                write_frame(&mut from_client, a).unwrap();
            }
            (hello, delivered.unwrap_or_default())
        });
        let mut client = TcpStream::connect(relay_addr).unwrap();
        client.write_all(&[MODE_RESUME]).unwrap();
        let cs = run_resume_initiator(
            &mut client,
            &f.ticket,
            &f.ca.anchor,
            &provider,
            NOW + 10,
            &mut SeedRng(client_seed),
        );
        drop(client);
        let ss = server.join().unwrap();
        let (hello, accept) = relay.join().unwrap();
        Run {
            client: cs,
            server: ss,
            hello,
            accept,
            replay_checks: replay_checks.load(Ordering::SeqCst),
        }
    })
}

fn untouched(_hello: &[u8], m: Vec<u8>) -> Vec<u8> {
    m
}

/// AC-52.3: two resumptions from one ticket derive different traffic secrets (each
/// agreed by both ends), and a ResumeAccept whose ciphertext is replaced by another
/// encapsulation to the same share fails the client's finished check.
#[test]
fn resumed_traffic_differs_when_the_kem_share_changes() {
    let f = fixture();
    let a = resume_via(&f, 10, &untouched, &untouched);
    let b = resume_via(&f, 11, &untouched, &untouched);
    let (ac, asrv) = (a.client.unwrap(), a.server.unwrap());
    let (bc, bsrv) = (b.client.unwrap(), b.server.unwrap());
    assert_eq!(ac.traffic_secret(), asrv.session.traffic_secret());
    assert_eq!(bc.traffic_secret(), bsrv.session.traffic_secret());
    assert_ne!(
        ac.traffic_secret(),
        bc.traffic_secret(),
        "a fresh KEM share gives a fresh traffic secret"
    );

    // An on-path party re-encapsulates to the client's share and splices its own
    // ciphertext in.
    let substitute = |hello: &[u8], mut accept: Vec<u8>| {
        let (suite, pk) = hello_kem_pk(hello);
        let pk = KemPublicKey::new(suite, hello[pk].to_vec());
        let (ct, _) = provider()
            .kem_for(suite)
            .unwrap()
            .encapsulate(&pk, &mut SeedRng(77))
            .unwrap();
        let range = accept_ct(&accept);
        assert_eq!(range.len(), ct.as_bytes().len());
        accept[range].copy_from_slice(ct.as_bytes());
        accept
    };
    let run = resume_via(&f, 12, &untouched, &substitute);
    assert!(run.server.is_ok(), "the responder saw a genuine resumption");
    assert!(matches!(run.client, Err(MtlsError::KeyConfirmationFailed)));
}

/// The binder covers the KEM share: swapping in another public key fails the
/// binder, and the responder never consults its replay guard, so the ticket is not
/// consumed.
#[test]
fn binder_covers_kem_share() {
    let f = fixture();
    let swap_share = |_: &[u8], mut hello: Vec<u8>| {
        let (suite, range) = hello_kem_pk(&hello);
        let (other, _) = provider()
            .kem_for(suite)
            .unwrap()
            .keypair(&mut SeedRng(78))
            .unwrap();
        assert_eq!(range.len(), other.as_bytes().len());
        hello[range].copy_from_slice(other.as_bytes());
        hello
    };
    let run = resume_via(&f, 13, &swap_share, &untouched);
    assert!(matches!(run.server, Err(MtlsError::KeyConfirmationFailed)));
    assert!(run.client.is_err());
    assert_eq!(
        run.replay_checks, 0,
        "a failed binder never touches the guard"
    );

    // The ticket still resumes afterwards.
    let ok = resume_via(&f, 14, &untouched, &untouched);
    assert!(ok.client.is_ok() && ok.server.is_ok());
    assert_eq!(ok.replay_checks, 1);
}

/// The resumed traffic secret is HKDF-SHA256 with salt = SHA-256(ResumeHello ||
/// ResumeAccept up to its finished MAC), which is public, and IKM = RMS || KEM
/// shared secret, which is not. The test rebuilds the client's ephemeral key from
/// its seed (the client draws its nonce, then its key pair), decapsulates the
/// observed ciphertext and recomputes the secret.
#[test]
fn hkdf_roles_salt_public_ikm_secret() {
    let f = fixture();
    let seed = 15;
    let run = resume_via(&f, seed, &untouched, &untouched);
    let session = run.client.unwrap();

    let (suite, pk_range) = hello_kem_pk(&run.hello);
    let kem = provider();
    let kem = kem.kem_for(suite).unwrap();
    let mut rng = SeedRng(seed);
    rng.fill(&mut [0u8; 32]);
    let (pk, sk) = kem.keypair(&mut rng).unwrap();
    assert_eq!(
        pk.as_bytes(),
        &run.hello[pk_range],
        "the client's share rebuilt"
    );
    let ct_range = accept_ct(&run.accept);
    let ct = Ciphertext::new(suite, run.accept[ct_range.clone()].to_vec());
    let ss = kem.decapsulate(&sk, &ct).unwrap();
    let rms = f.client.resumption_secret();

    let mut transcript = run.hello.clone();
    transcript.extend_from_slice(&run.accept[..ct_range.end]);
    let salt = sha256(&transcript);
    let mut ikm = rms.expose().to_vec();
    ikm.extend_from_slice(ss.expose());
    assert_eq!(
        session.traffic_secret(),
        &hkdf_sha256_32(&salt, &ikm, RESUME_INFO),
        "salt = transcript hash, IKM = RMS || shared secret"
    );

    // Everything an observer sees, plus the RMS, is not enough without the shared
    // secret, and the RMS is not in the salt role.
    let mut without_ss = rms.expose().to_vec();
    without_ss.extend_from_slice(&[0u8; 32]);
    assert_ne!(
        session.traffic_secret(),
        &hkdf_sha256_32(&salt, &without_ss, RESUME_INFO)
    );
    assert_ne!(
        session.traffic_secret(),
        &hkdf_sha256_32(rms.expose(), &transcript, RESUME_INFO)
    );
}
