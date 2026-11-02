//! The ClientHello is the first frame a mesh listener reads, before any certificate is
//! checked, so its decoder must refuse a hostile count without sizing an allocation
//! from it: a suite or signature-id count larger than the frame can hold, or longer
//! than any real peer advertises, is malformed, and promptly. A frame length above the
//! limit is refused before the body is read, and a real ClientHello still round-trips.
//! [VLD-ANB-MIDGARD-0010] [AC-35.1] [AC-35.2] [AC-35.4] [NFR-SEC-1]
//!
//! The negative paths of the whole handshake codec are proved on frames recorded from
//! one live handshake: every truncation and every trailing byte of each of the four
//! messages is refused, the frame readers hold their limits, a recorded
//! ClientKeyExchange replayed to a fresh responder fails its signature, and an unknown
//! tag or certificate-presence byte is malformed. Like the tests above, these run on
//! either backend through the shared `common` fixtures.
//! [VLD-ANB-MIDGARD-0093] [AC-42.3] [AC-42.4]
#![cfg(any(feature = "test-backend", feature = "backend-fips"))]

mod common;

use std::io::Cursor;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use common::SeedRng;
use policy::{ClassificationLevel, DefaultPdp, Purpose};
use pq_mtls::{
    decode_message, encode_message, read_frame, read_handshake_frame, run_initiator, run_responder,
    write_frame, Channel, ClientHello, HandshakeMessage, Initiator, MtlsError, Posture, Responder,
    TransportError, MAX_HANDSHAKE_FRAME, MAX_RECORD_FRAME, PROTOCOL_VERSION,
};
use pqc_agility::{Capabilities, KemSuiteId, SigId};

/// Where the KEM-suite count sits in a ClientHello: after the tag, the version and the
/// 32-byte nonce.
const SUITE_COUNT_AT: usize = 1 + 1 + 32;

/// A real ClientHello, as the initiator sends it.
fn client_hello() -> ClientHello {
    let ca = common::ca(1);
    let client = common::party("analyst-svc", &["analyst"], 10, &ca);
    let server = common::party("gateway", &["server"], 20, &ca);
    let provider = common::provider();
    let mut crng = SeedRng::new(1);
    let mut init = Initiator::new(
        &provider,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        server.cert.identity.id.clone(),
        Purpose("investigation".into()),
        Posture::unclassified(),
        &mut crng,
    );
    init.start()
}

fn client_hello_bytes() -> Vec<u8> {
    encode_message(&HandshakeMessage::ClientHello(client_hello()))
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// `body` with its KEM-suite count replaced by `nk` and its signature-id count (which
/// follows the suite bytes actually present) by `ns`.
fn with_counts(body: &[u8], nk: u32, ns: u32) -> Vec<u8> {
    let mut b = body.to_vec();
    let suites = u32_at(&b, SUITE_COUNT_AT) as usize;
    let sig_count_at = SUITE_COUNT_AT + 4 + suites;
    b[SUITE_COUNT_AT..SUITE_COUNT_AT + 4].copy_from_slice(&nk.to_le_bytes());
    b[sig_count_at..sig_count_at + 4].copy_from_slice(&ns.to_le_bytes());
    b
}

/// Decode `body`, asserting it is refused as malformed and promptly.
fn assert_refused_promptly(body: &[u8]) {
    let started = Instant::now();
    let result = decode_message(body);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the refusal took {:?}",
        started.elapsed()
    );
    assert!(
        matches!(result, Err(TransportError::Malformed(_))),
        "a hostile count was not refused: {result:?}"
    );
}

#[test]
fn a_client_hello_with_a_hostile_suite_count_is_refused() {
    let body = client_hello_bytes();
    let suites = u32_at(&body, SUITE_COUNT_AT);
    let sigs = u32_at(&body, SUITE_COUNT_AT + 4 + suites as usize);
    assert_refused_promptly(&with_counts(&body, u32::MAX, sigs));
    assert_refused_promptly(&with_counts(&body, suites, u32::MAX));
}

#[test]
fn a_client_hello_with_seventeen_suites_is_refused() {
    let mut hello = client_hello();
    hello.capabilities.kem_suites = vec![KemSuiteId::HybridX25519MlKem768; 17];
    let body = encode_message(&HandshakeMessage::ClientHello(hello));
    match decode_message(&body) {
        Err(TransportError::Malformed(m)) => assert!(m.contains("too many capability entries")),
        other => panic!("seventeen suites were not refused: {other:?}"),
    }
}

#[test]
fn a_frame_length_above_max_frame_is_refused_before_reading() {
    match read_frame(&mut Cursor::new(0xFFFF_FFFFu32.to_le_bytes())) {
        Err(TransportError::Malformed(m)) => assert!(m.contains("frame too large")),
        other => panic!("an oversized frame was not refused: {other:?}"),
    }
}

#[test]
fn a_round_trip_client_hello_still_decodes() {
    let capabilities = Capabilities {
        kem_suites: vec![KemSuiteId::HybridX25519MlKem768, KemSuiteId::MlKem1024],
        sig_algs: vec![SigId::MlDsa65, SigId::MlDsa87],
    };
    let hello = ClientHello {
        capabilities: capabilities.clone(),
        ..client_hello()
    };
    assert_eq!(hello.version, PROTOCOL_VERSION);
    match decode_message(&encode_message(&HandshakeMessage::ClientHello(hello))) {
        Ok(HandshakeMessage::ClientHello(back)) => assert_eq!(back.capabilities, capabilities),
        other => panic!("a real ClientHello did not decode: {other:?}"),
    }
}

// ---- frames recorded from a live handshake [AC-42.3] [AC-42.4] --------------------------

/// One end of a channel that carries each message as its encoded frame body, keeping a
/// copy of every body sent by either end in `log`. The receiving end decodes the bytes,
/// so the recorded bodies are exactly what crossed the wire.
struct Recording {
    tx: Sender<Vec<u8>>,
    rx: Receiver<Vec<u8>>,
    log: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Channel for Recording {
    fn send(&mut self, msg: HandshakeMessage) -> Result<(), TransportError> {
        let body = encode_message(&msg);
        self.log.lock().expect("log").push(body.clone());
        self.tx.send(body).map_err(|_| TransportError::Closed)
    }
    fn recv(&mut self) -> Result<HandshakeMessage, TransportError> {
        let body = self.rx.recv().map_err(|_| TransportError::Closed)?;
        decode_message(&body)
    }
}

/// The parties of the recorded handshake: (CA, client, server).
fn parties() -> (common::Ca, common::Party, common::Party) {
    let ca = common::ca(1);
    let client = common::party("analyst-svc", &["analyst"], 10, &ca);
    let server = common::party("gateway", &["server"], 20, &ca);
    (ca, client, server)
}

/// A responder for `server` that draws its nonce and KEM key from `rng`.
fn responder<'a>(
    provider: &'a pqc_agility::CryptoProvider,
    server: &'a common::Party,
    ca: &common::Ca,
    pdp: &'a DefaultPdp,
    rng: &'a mut SeedRng,
) -> Responder<'a> {
    Responder::new(
        provider,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        pdp,
        common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]),
        rng,
    )
}

/// Run one successful handshake between the two channel drivers over a pair of
/// recording channels, and return the four frame bodies in the order they were sent:
/// ClientHello, ServerHello, ClientKeyExchange, Finished.
fn recorded_handshake(
    ca: &common::Ca,
    client: &common::Party,
    server: &common::Party,
) -> Vec<Vec<u8>> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (to_server, from_client) = mpsc::channel();
    let (to_client, from_server) = mpsc::channel();
    let client_end = Recording {
        tx: to_server,
        rx: from_server,
        log: log.clone(),
    };
    let mut server_end = Recording {
        tx: to_client,
        rx: from_client,
        log: log.clone(),
    };
    thread::scope(|s| {
        let server_side = s.spawn(move || {
            let provider = common::provider();
            let pdp = common::permit_pdp("analyst");
            let mut srng = SeedRng::new(2);
            let resp = responder(&provider, server, ca, &pdp, &mut srng);
            run_responder(resp, &mut server_end).map(|_| ())
        });
        // Owned by this closure, so a failed handshake drops it and the server side
        // stops waiting instead of hanging the scope.
        let mut client_end = client_end;
        let provider = common::provider();
        let mut crng = SeedRng::new(1);
        let init = Initiator::new(
            &provider,
            client.cert.clone(),
            &client.signing_key,
            ca.anchor.clone(),
            server.cert.identity.id.clone(),
            Purpose("investigation".into()),
            Posture::unclassified(),
            &mut crng,
        );
        run_initiator(init, &mut client_end).expect("the recorded handshake succeeds");
        drop(client_end);
        server_side
            .join()
            .expect("server side")
            .expect("the responder completes");
    });
    let frames = log.lock().expect("log").clone();
    assert_eq!(frames.len(), 4, "one handshake is four frames");
    frames
}

/// The variant a message decodes to, for comparison.
fn kind(m: &HandshakeMessage) -> &'static str {
    match m {
        HandshakeMessage::ClientHello(_) => "ClientHello",
        HandshakeMessage::ServerHello(_) => "ServerHello",
        HandshakeMessage::ClientKeyExchange(_) => "ClientKeyExchange",
        HandshakeMessage::Finished(_) => "Finished",
    }
}

const ORDER: [&str; 4] = [
    "ClientHello",
    "ServerHello",
    "ClientKeyExchange",
    "Finished",
];

#[test]
fn every_truncation_of_every_message_is_refused() {
    let (ca, client, server) = parties();
    let frames = recorded_handshake(&ca, &client, &server);
    for (body, expected) in frames.iter().zip(ORDER) {
        // The full body still decodes, to the message it was, and re-encodes canonically.
        let msg = decode_message(body).expect("a recorded body decodes");
        assert_eq!(kind(&msg), expected);
        assert_eq!(&encode_message(&msg), body, "{expected} is canonical");
        // Every proper prefix is refused.
        for len in 0..body.len() {
            assert!(
                decode_message(&body[..len]).is_err(),
                "{expected} truncated to {len} of {} bytes was accepted",
                body.len()
            );
        }
        // So is the body with one byte more.
        match decode_message(&[body.as_slice(), &[0u8]].concat()) {
            Err(TransportError::Malformed(m)) => assert!(m.contains("trailing"), "{m}"),
            other => panic!("{expected} with a trailing byte was not refused: {other:?}"),
        }
    }
}

#[test]
fn frames_above_max_frame_or_short_of_their_prefix_are_refused() {
    // The limits are checked on the length prefix, before any body is read. (The
    // MG-SEC-13 tests in sec_hardening.rs prove this for `read_frame_limited` with an
    // arbitrary limit; these pin the two readers the transport uses at their exact
    // boundaries.)
    let over_record = ((MAX_RECORD_FRAME + 1) as u32).to_le_bytes();
    match read_frame(&mut Cursor::new(over_record)) {
        Err(TransportError::Malformed(m)) => assert!(m.contains("frame too large"), "{m}"),
        other => panic!("a frame one byte over MAX_RECORD_FRAME was read: {other:?}"),
    }
    let over_handshake = ((MAX_HANDSHAKE_FRAME + 1) as u32).to_le_bytes();
    match read_handshake_frame(&mut Cursor::new(over_handshake)) {
        Err(TransportError::Malformed(m)) => assert!(m.contains("frame too large"), "{m}"),
        other => panic!("a frame one byte over MAX_HANDSHAKE_FRAME was read: {other:?}"),
    }
    // A prefix that promises more than the connection delivers is an I/O error.
    let mut short = 100u32.to_le_bytes().to_vec();
    short.extend_from_slice(&[7; 10]);
    assert!(matches!(
        read_frame(&mut Cursor::new(short)),
        Err(TransportError::Io(_))
    ));
    // A prefix equal to a real body's length, followed by that body, round-trips.
    let (ca, client, server) = parties();
    for body in recorded_handshake(&ca, &client, &server) {
        let mut wire = Vec::new();
        write_frame(&mut wire, &body).unwrap();
        assert_eq!(u32_at(&wire, 0) as usize, body.len());
        let back = read_handshake_frame(&mut Cursor::new(wire)).expect("a real frame reads");
        assert_eq!(back, body);
    }
}

#[test]
fn a_replayed_client_key_exchange_is_refused() {
    let (ca, client, server) = parties();
    let frames = recorded_handshake(&ca, &client, &server);
    let Ok(HandshakeMessage::ClientHello(hello)) = decode_message(&frames[0]) else {
        panic!("frame 0 is the ClientHello");
    };
    let Ok(HandshakeMessage::ServerHello(recorded_sh)) = decode_message(&frames[1]) else {
        panic!("frame 1 is the ServerHello");
    };
    let Ok(HandshakeMessage::ClientKeyExchange(cke)) = decode_message(&frames[2]) else {
        panic!("frame 2 is the ClientKeyExchange");
    };

    // A fresh responder, with its own nonce and KEM key. (The recorded responder drew
    // from seed 2; a production responder draws from the OS.)
    let provider = common::provider();
    let pdp = common::permit_pdp("analyst");
    let mut srng = SeedRng::new(3);
    let mut fresh = responder(&provider, &server, &ca, &pdp, &mut srng);
    let sh = fresh
        .on_client_hello(hello)
        .expect("a replayed ClientHello is answered");
    assert_ne!(sh.nonce_s, recorded_sh.nonce_s, "the fresh nonce is new");

    // The client's signature covers a transcript holding the old server nonce, so the
    // replayed key exchange fails on its signature, not on a missing credential.
    match fresh.on_client_key_exchange(cke) {
        Err(MtlsError::PeerSignatureInvalid) => {}
        Err(MtlsError::MissingPeerCredential) => {
            panic!("refused for the wrong reason: the recorded CKE carries its certificate")
        }
        Err(other) => panic!("refused, but not on the signature: {other:?}"),
        Ok(_) => panic!("a replayed ClientKeyExchange established a session"),
    }
}

#[test]
fn an_unknown_tag_and_a_bad_cert_presence_byte_are_refused() {
    let (ca, client, server) = parties();
    let frames = recorded_handshake(&ca, &client, &server);

    let mut unknown = frames[0].clone();
    unknown[0] = 0x7F;
    for body in [vec![0x7F], unknown] {
        match decode_message(&body) {
            Err(TransportError::Malformed(m)) => assert!(m.contains("unknown message tag"), "{m}"),
            other => panic!("tag 0x7F was not refused: {other:?}"),
        }
    }

    // A ClientKeyExchange is the tag, the KEM suite, the u32 ciphertext length and the
    // ciphertext, then the certificate-presence byte (0 absent, 1 a version 2
    // certificate, 2 a version 3 one): 3 is none of them.
    let mut cke = frames[2].clone();
    let presence_at = 1 + 1 + 4 + u32_at(&cke, 2) as usize;
    assert_eq!(
        cke[presence_at], 1,
        "the recorded CKE carries its certificate"
    );
    cke[presence_at] = 3;
    match decode_message(&cke) {
        Err(TransportError::Malformed(m)) => assert!(m.contains("bad cert presence"), "{m}"),
        other => panic!("a presence byte of 3 was not refused: {other:?}"),
    }
}
