//! The ClientHello is the first frame a mesh listener reads, before any certificate is
//! checked, so its decoder must refuse a hostile count without sizing an allocation
//! from it: a suite or signature-id count larger than the frame can hold, or longer
//! than any real peer advertises, is malformed, and promptly. A frame length above the
//! limit is refused before the body is read, and a real ClientHello still round-trips.
//! [VLD-ANB-MIDGARD-0010] [AC-35.1] [AC-35.2] [AC-35.4] [NFR-SEC-1]
#![cfg(any(feature = "test-backend", feature = "backend-fips"))]

mod common;

use std::io::Cursor;
use std::time::{Duration, Instant};

use common::SeedRng;
use policy::Purpose;
use pq_mtls::{
    decode_message, encode_message, read_frame, ClientHello, HandshakeMessage, Initiator, Posture,
    TransportError, PROTOCOL_VERSION,
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
