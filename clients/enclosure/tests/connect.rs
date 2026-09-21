//! AC-C-3 — client-to-enclosure transport is hybrid PQ-mTLS and fails closed if PQ
//! negotiation fails. Driven over real TCP sockets, the transport a shell uses.
#![cfg(feature = "test-backend")]

mod common;

use std::net::{TcpListener, TcpStream};
use std::thread;

use policy::Purpose;
use pq_mtls::{run_responder, Posture, Responder, ServiceId, TcpChannel};
use pqc_agility::{KemSuiteId, SeedRng};

use midgard_client_enclosure::{EnclosureClient, EnclosureError};

/// Happy path: the client opens a hybrid PQ-mTLS session to the pinned enclosure
/// gateway, and the negotiated suite is post-quantum.
#[test]
fn connects_over_hybrid_pq_mtls_to_the_pinned_gateway() {
    let provider = common::provider();
    let ca = common::ca(1);
    let client = common::party("device-client", &["analyst"], 10, &ca);
    let gateway = common::party("enclosure-gateway", &["server"], 20, &ca);
    let pdp = common::permit_pdp("analyst");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::scope(|s| {
        // The enclosure gateway (responder).
        let server = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            let mut ch = TcpChannel::new(stream);
            let mut srng = SeedRng::new(2);
            let resp = Responder::new(
                &provider,
                gateway.cert.clone(),
                &gateway.signing_key,
                ca.anchor.clone(),
                &pdp,
                common::channel_resource(),
                &mut srng,
            );
            let session = run_responder(resp, &mut ch).expect("gateway handshake");
            assert!(session.suite().is_post_quantum());
        });

        // The client.
        let client_cfg = EnclosureClient::new(
            client.cert.clone(),
            ca.anchor.clone(),
            Purpose("investigation".into()),
            Posture::unclassified(),
        )
        .pin_peer(ServiceId("enclosure-gateway".into()));

        let stream = TcpStream::connect(addr).unwrap();
        let mut ch = TcpChannel::new(stream);
        let mut crng = SeedRng::new(1);
        let session = client_cfg
            .connect(&provider, &client.signing_key, &mut crng, &mut ch)
            .expect("client session");

        assert!(session.suite().is_post_quantum(), "session must be PQ");
        assert_eq!(session.suite(), KemSuiteId::HybridX25519MlKem768);
        assert_eq!(session.peer().id, ServiceId("enclosure-gateway".into()));

        server.join().unwrap();
    });
}

/// Fail closed: a client that can offer no post-quantum suite gets no session.
#[test]
fn no_common_pq_suite_fails_closed() {
    let gateway_provider = common::provider();
    let client_provider = common::provider_without_kems();
    let ca = common::ca(3);
    let client = common::party("device-client", &["analyst"], 30, &ca);
    let gateway = common::party("enclosure-gateway", &["server"], 40, &ca);
    let pdp = common::permit_pdp("analyst");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::scope(|s| {
        let server = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            let mut ch = TcpChannel::new(stream);
            let mut srng = SeedRng::new(2);
            let resp = Responder::new(
                &gateway_provider,
                gateway.cert.clone(),
                &gateway.signing_key,
                ca.anchor.clone(),
                &pdp,
                common::channel_resource(),
                &mut srng,
            );
            // The gateway cannot negotiate a PQ suite → fails closed.
            let _ = run_responder(resp, &mut ch);
        });

        let client_cfg = EnclosureClient::new(
            client.cert.clone(),
            ca.anchor.clone(),
            Purpose("investigation".into()),
            Posture::unclassified(),
        );

        let stream = TcpStream::connect(addr).unwrap();
        let mut ch = TcpChannel::new(stream);
        let mut crng = SeedRng::new(1);
        let result = client_cfg.connect(&client_provider, &client.signing_key, &mut crng, &mut ch);

        assert!(result.is_err(), "no PQ suite must yield no session");
        assert!(matches!(result, Err(EnclosureError::Handshake(_))));

        server.join().unwrap();
    });
}

/// Fail closed: even after a valid handshake, a server other than the pinned
/// gateway is rejected — the client never accepts an unexpected enclosure peer.
#[test]
fn a_wrong_peer_is_rejected_by_the_pin() {
    let provider = common::provider();
    let ca = common::ca(5);
    let client = common::party("device-client", &["analyst"], 50, &ca);
    // The server authenticates fine, but is not the gateway the client pinned.
    let other = common::party("some-other-service", &["server"], 60, &ca);
    let pdp = common::permit_pdp("analyst");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::scope(|s| {
        let server = s.spawn(|| {
            let (stream, _) = listener.accept().unwrap();
            let mut ch = TcpChannel::new(stream);
            let mut srng = SeedRng::new(2);
            let resp = Responder::new(
                &provider,
                other.cert.clone(),
                &other.signing_key,
                ca.anchor.clone(),
                &pdp,
                common::channel_resource(),
                &mut srng,
            );
            let _ = run_responder(resp, &mut ch);
        });

        let client_cfg = EnclosureClient::new(
            client.cert.clone(),
            ca.anchor.clone(),
            Purpose("investigation".into()),
            Posture::unclassified(),
        )
        .pin_peer(ServiceId("enclosure-gateway".into()));

        let stream = TcpStream::connect(addr).unwrap();
        let mut ch = TcpChannel::new(stream);
        let mut crng = SeedRng::new(1);
        let result = client_cfg.connect(&provider, &client.signing_key, &mut crng, &mut ch);

        assert!(matches!(
            result,
            Err(EnclosureError::PeerPinMismatch { .. })
        ));

        server.join().unwrap();
    });
}
