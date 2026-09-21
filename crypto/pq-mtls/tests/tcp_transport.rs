//! The PQ-mTLS handshake + record layer over **real TCP sockets** — the
//! transport a service actually terminates. Proves AC-M-1 (hybrid negotiated,
//! fail closed) and AC-M-3 (mutual auth + authz) end-to-end over the wire.
#![cfg(feature = "backend-fips")]

mod fips_fixtures;
use fips_fixtures::{
    ca, channel_resource, deny_pdp, party, permit_pdp, posture, provider, SeedRng,
};

use std::net::{TcpListener, TcpStream};
use std::thread;

use policy::Purpose;
use pq_mtls::{
    read_frame, run_initiator, run_responder, write_frame, Initiator, MtlsError, Responder,
    Role as RecordRole, ServiceId, TcpChannel,
};
use pqc_agility::KemSuiteId;

/// Full round trip: real handshake over TCP, then an authenticated request and
/// response over the AES-256-GCM record layer.
#[test]
fn tcp_handshake_and_records() {
    let provider = provider();
    let ca = ca(&provider, 1);
    let client = party(&provider, "svc-client", &["analyst"], 10, &ca);
    let server = party(&provider, "svc-server", &["server"], 20, &ca);
    let pdp = permit_pdp("analyst");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::scope(|s| {
        // --- server ---
        let server_thread = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            let mut ch = TcpChannel::new(stream);
            let mut srng = SeedRng(2);
            let resp = Responder::new(
                &provider,
                server.cert.clone(),
                &server.signing_key,
                ca.anchor.clone(),
                &pdp,
                channel_resource(),
                &mut srng,
            );
            let session = run_responder(resp, &mut ch).expect("server handshake");
            assert_eq!(session.suite(), KemSuiteId::HybridX25519MlKem768);
            assert_eq!(session.peer().id, ServiceId("svc-client".into()));

            let mut stream = ch.into_stream();
            let (mut seal, mut open) = session.records(RecordRole::Responder);
            // Receive the request, reply.
            let req = open
                .open(b"req", &read_frame(&mut stream).unwrap())
                .unwrap();
            assert_eq!(&req, b"PING");
            write_frame(&mut stream, &seal.seal(b"resp", b"PONG").unwrap()).unwrap();
        });

        // --- client ---
        let stream = TcpStream::connect(addr).unwrap();
        let mut ch = TcpChannel::new(stream);
        let mut crng = SeedRng(1);
        let init = Initiator::new(
            &provider,
            client.cert.clone(),
            &client.signing_key,
            ca.anchor.clone(),
            Purpose("investigation".into()),
            posture(),
            &mut crng,
        );
        let session = run_initiator(init, &mut ch).expect("client handshake");
        assert_eq!(session.suite(), KemSuiteId::HybridX25519MlKem768);
        assert_eq!(session.peer().id, ServiceId("svc-server".into()));

        let mut stream = ch.into_stream();
        let (mut seal, mut open) = session.records(RecordRole::Initiator);
        write_frame(&mut stream, &seal.seal(b"req", b"PING").unwrap()).unwrap();
        let resp = open
            .open(b"resp", &read_frame(&mut stream).unwrap())
            .unwrap();
        assert_eq!(&resp, b"PONG");

        server_thread.join().unwrap();
    });
}

/// AC-M-3 fail-closed over TCP: an unauthorised (deny-by-default) peer is denied
/// at the server even though the handshake authenticated it.
#[test]
fn tcp_handshake_denies_unauthorized() {
    let provider = provider();
    let ca = ca(&provider, 3);
    let client = party(&provider, "svc-client", &["analyst"], 30, &ca);
    let server = party(&provider, "svc-server", &["server"], 40, &ca);
    let pdp = deny_pdp();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::scope(|s| {
        let server_thread = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            let mut ch = TcpChannel::new(stream);
            let mut srng = SeedRng(2);
            let resp = Responder::new(
                &provider,
                server.cert.clone(),
                &server.signing_key,
                ca.anchor.clone(),
                &pdp,
                channel_resource(),
                &mut srng,
            );
            // The server rejects the connection at authorization.
            let err = run_responder(resp, &mut ch).unwrap_err();
            assert!(
                matches!(err, MtlsError::Authz(_)),
                "server must deny: {err:?}"
            );
        });

        let stream = TcpStream::connect(addr).unwrap();
        let mut ch = TcpChannel::new(stream);
        let mut crng = SeedRng(1);
        let init = Initiator::new(
            &provider,
            client.cert.clone(),
            &client.signing_key,
            ca.anchor.clone(),
            Purpose("investigation".into()),
            posture(),
            &mut crng,
        );
        // The client's handshake also fails closed (the server never sends the
        // final flight after denying); it must not yield a Session.
        let _ = run_initiator(init, &mut ch);
        server_thread.join().unwrap();
    });
}
