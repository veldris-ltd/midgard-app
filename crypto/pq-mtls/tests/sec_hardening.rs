//! The October 2026 hardening of the handshake, each property proved fail-closed:
//! the initiator only talks to the service it named [MG-SEC-01], certificates expire
//! [MG-SEC-02], revocations reach every clone of an anchor [MG-SEC-03], clearance comes
//! from the certificate [MG-SEC-04], a peer on another protocol version is refused
//! [MG-SEC-09], hybrid is required by default [MG-SEC-12], and hostile lengths never
//! size an allocation [MG-SEC-13].
#![cfg(feature = "test-backend")]

mod common;

use std::collections::BTreeSet;
use std::io::Cursor;

use policy::{ClassificationLevel, Compartment, Purpose};
use pq_mtls::{
    certify, decode_message, encode_message, mutual_handshake, read_frame_limited, CertTerms,
    ClientHello, HandshakeMessage, Initiator, MtlsError, Posture, Responder, TransportError,
    PROTOCOL_VERSION,
};
use pqc_agility::{KemSuiteId, SeedRng};

const NOW: u64 = 1_800_000_000;

fn posture(level: ClassificationLevel, compartments: &[&str]) -> Posture {
    Posture {
        level,
        compartments: compartments
            .iter()
            .map(|c| Compartment(c.to_string()))
            .collect(),
    }
}

/// A party certified with explicit terms.
fn party_with(
    id: &str,
    roles: &[&str],
    seed: u64,
    ca: &common::Ca,
    terms: CertTerms,
) -> common::Party {
    let mut p = common::party(id, roles, seed, ca);
    p.cert = certify(
        &common::provider(),
        common::ALG,
        &ca.signing_key,
        &p.identity,
        terms,
    )
    .unwrap();
    p
}

struct Outcome {
    result: Result<(), MtlsError>,
    clearance: Option<Posture>,
}

/// Run a full in-process handshake. `expect` is the service the client means to
/// reach; `requested` the posture it asks for.
#[allow(clippy::too_many_arguments)]
fn handshake(
    client: &common::Party,
    server: &common::Party,
    ca: &common::Ca,
    expect: &str,
    requested: Posture,
    resource_level: ClassificationLevel,
    now: u64,
    client_provider: &pqc_agility::CryptoProvider,
) -> Outcome {
    let provider = common::provider();
    let pdp = common::permit_pdp("analyst");
    let mut resource = common::channel_resource(resource_level, &["investigation"]);
    resource.marking.compartments = BTreeSet::new();
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
    let mut init = Initiator::new(
        client_provider,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        pq_mtls::ServiceId(expect.into()),
        Purpose("investigation".into()),
        requested,
        &mut crng,
    )
    .at_time(now);
    let mut resp = Responder::new(
        &provider,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        resource,
        &mut srng,
    )
    .at_time(now);
    match mutual_handshake(&mut init, &mut resp) {
        Ok((_, s)) => Outcome {
            result: Ok(()),
            clearance: s.authorization().map(|a| a.clearance.clone()),
        },
        Err(e) => Outcome {
            result: Err(e),
            clearance: None,
        },
    }
}

fn fixtures() -> (common::Ca, common::Party, common::Party) {
    let ca = common::ca(1);
    let client = common::party("analyst-svc", &["analyst"], 10, &ca);
    let server = common::party("gateway", &["server"], 20, &ca);
    (ca, client, server)
}

// ---- MG-SEC-01 -------------------------------------------------------------

#[test]
fn a_valid_certificate_for_another_service_is_refused_before_the_key_exchange() {
    let ca = common::ca(1);
    let client = common::party("analyst-svc", &["analyst"], 10, &ca);
    // A stolen edge-device certificate: valid, same CA, wrong service.
    let impostor = common::party("edge-device-7", &["server"], 30, &ca);
    let provider = common::provider();
    let pdp = common::permit_pdp("analyst");
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
    let mut init = Initiator::new(
        &provider,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        pq_mtls::ServiceId("gateway".into()),
        Purpose("investigation".into()),
        Posture::unclassified(),
        &mut crng,
    );
    let mut resp = Responder::new(
        &provider,
        impostor.cert.clone(),
        &impostor.signing_key,
        ca.anchor.clone(),
        &pdp,
        common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]),
        &mut srng,
    );
    let hello = init.start();
    let server_hello = resp.on_client_hello(hello).unwrap();
    // The initiator refuses at the ServerHello: it never produces a key exchange, so
    // nothing — not even its own certificate — reaches the impostor.
    match init.on_server_hello(server_hello) {
        Err(MtlsError::PeerIdentityMismatch { expected, got }) => {
            assert_eq!(expected, "gateway");
            assert_eq!(got, "edge-device-7");
        }
        other => panic!("expected PeerIdentityMismatch, got {other:?}"),
    }
}

#[test]
fn the_named_service_is_reached() {
    let (ca, client, server) = fixtures();
    let o = handshake(
        &client,
        &server,
        &ca,
        "gateway",
        Posture::unclassified(),
        ClassificationLevel::UNCLASSIFIED,
        NOW,
        &common::provider(),
    );
    o.result.unwrap();
}

// ---- MG-SEC-02 -------------------------------------------------------------

#[test]
fn an_expired_certificate_fails_the_handshake_on_either_side() {
    let ca = common::ca(1);
    let window = |serial| {
        CertTerms::new(
            serial,
            NOW - 100,
            NOW,
            posture(ClassificationLevel::TOP_SECRET, &[]),
        )
    };
    let fresh_client = common::party("analyst-svc", &["analyst"], 10, &ca);
    let fresh_server = common::party("gateway", &["server"], 20, &ca);
    let old_client = party_with("analyst-svc", &["analyst"], 11, &ca, window(1));
    let old_server = party_with("gateway", &["server"], 21, &ca, window(2));

    for (client, server) in [(&old_client, &fresh_server), (&fresh_client, &old_server)] {
        // Inside the window it works …
        let o = handshake(
            client,
            server,
            &ca,
            "gateway",
            Posture::unclassified(),
            ClassificationLevel::UNCLASSIFIED,
            NOW - 1,
            &common::provider(),
        );
        o.result.unwrap();
        // … at not_after it does not.
        let o = handshake(
            client,
            server,
            &ca,
            "gateway",
            Posture::unclassified(),
            ClassificationLevel::UNCLASSIFIED,
            NOW,
            &common::provider(),
        );
        assert!(
            matches!(o.result, Err(MtlsError::PeerCertExpired { .. })),
            "{:?}",
            o.result
        );
    }
}

// ---- MG-SEC-03 -------------------------------------------------------------

#[test]
fn a_revocation_reaches_anchors_already_handed_out() {
    let (ca, client, server) = fixtures();
    let in_use = ca.anchor.clone();
    ca.anchor.revoke(client.cert.fingerprint());
    assert!(in_use.is_revoked(&client.cert.fingerprint()));
    let o = handshake(
        &client,
        &server,
        &ca,
        "gateway",
        Posture::unclassified(),
        ClassificationLevel::UNCLASSIFIED,
        NOW,
        &common::provider(),
    );
    assert!(matches!(o.result, Err(MtlsError::PeerCertRevoked)));
}

// ---- MG-SEC-04 -------------------------------------------------------------

#[test]
fn claiming_a_higher_clearance_than_the_certificate_grants_does_not_work() {
    let ca = common::ca(1);
    let grant = CertTerms::new(
        1,
        0,
        u64::MAX,
        posture(ClassificationLevel::UNCLASSIFIED, &[]),
    );
    let client = party_with("analyst-svc", &["analyst"], 10, &ca, grant);
    let server = common::party("gateway", &["server"], 20, &ca);
    // Asks for SECRET to reach a SECRET channel: refused, it holds UNCLASSIFIED.
    let o = handshake(
        &client,
        &server,
        &ca,
        "gateway",
        posture(ClassificationLevel::SECRET, &[]),
        ClassificationLevel::SECRET,
        NOW,
        &common::provider(),
    );
    assert!(
        matches!(o.result, Err(MtlsError::Authz(_))),
        "{:?}",
        o.result
    );
}

#[test]
fn the_effective_clearance_is_the_lower_of_grant_and_request() {
    let ca = common::ca(1);
    let grant = CertTerms::new(
        1,
        0,
        u64::MAX,
        posture(ClassificationLevel::SECRET, &["alpha", "bravo"]),
    );
    let client = party_with("analyst-svc", &["analyst"], 10, &ca, grant);
    let server = common::party("gateway", &["server"], 20, &ca);

    // Requests more than granted: held to SECRET, and a compartment it was never
    // granted is dropped.
    let o = handshake(
        &client,
        &server,
        &ca,
        "gateway",
        posture(ClassificationLevel::TOP_SECRET, &["alpha", "charlie"]),
        ClassificationLevel::UNCLASSIFIED,
        NOW,
        &common::provider(),
    );
    o.result.unwrap();
    assert_eq!(
        o.clearance.unwrap(),
        posture(ClassificationLevel::SECRET, &["alpha"])
    );

    // Requests less than granted: gets what it asked for.
    let o = handshake(
        &client,
        &server,
        &ca,
        "gateway",
        posture(ClassificationLevel::CONFIDENTIAL, &[]),
        ClassificationLevel::UNCLASSIFIED,
        NOW,
        &common::provider(),
    );
    assert_eq!(
        o.clearance.unwrap(),
        posture(ClassificationLevel::CONFIDENTIAL, &[])
    );
}

// ---- MG-SEC-09 / MG-SEC-12 -------------------------------------------------

#[test]
fn another_protocol_version_is_refused() {
    let (ca, client, server) = fixtures();
    let provider = common::provider();
    let pdp = common::permit_pdp("analyst");
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
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
    let mut resp = Responder::new(
        &provider,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]),
        &mut srng,
    );
    let mut hello: ClientHello = init.start();
    assert_eq!(hello.version, PROTOCOL_VERSION);
    hello.version = 1;
    assert!(matches!(
        resp.on_client_hello(hello),
        Err(MtlsError::UnsupportedVersion(1))
    ));
}

/// A client that only offers pure ML-KEM suites.
fn pure_mlkem_only() -> pqc_agility::CryptoProvider {
    let mut p = common::provider();
    p.disable_kem(KemSuiteId::HybridX25519MlKem768);
    p.disable_kem(KemSuiteId::HybridX25519MlKem1024);
    p
}

#[test]
fn hybrid_is_required_by_default() {
    let (ca, client, server) = fixtures();
    let pure = pure_mlkem_only();
    // The client must opt out to even offer pure ML-KEM; the server still refuses.
    let provider = common::provider();
    let pdp = common::permit_pdp("analyst");
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
    let mut init = Initiator::new(
        &pure,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        server.cert.identity.id.clone(),
        Purpose("investigation".into()),
        Posture::unclassified(),
        &mut crng,
    )
    .allow_non_hybrid();
    let mut resp = Responder::new(
        &provider,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]),
        &mut srng,
    );
    assert!(matches!(
        mutual_handshake(&mut init, &mut resp),
        Err(MtlsError::HybridRequired)
    ));

    // Every default handshake negotiates a hybrid suite.
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
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
    let mut resp = Responder::new(
        &provider,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]),
        &mut srng,
    );
    let (c, s) = mutual_handshake(&mut init, &mut resp).unwrap();
    assert!(c.suite().is_hybrid() && s.suite().is_hybrid());
}

#[test]
fn pure_mlkem_only_with_both_sides_opted_out() {
    let (ca, client, server) = fixtures();
    let pure = pure_mlkem_only();
    let pdp = common::permit_pdp("analyst");
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
    let mut init = Initiator::new(
        &pure,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        server.cert.identity.id.clone(),
        Purpose("investigation".into()),
        Posture::unclassified(),
        &mut crng,
    )
    .allow_non_hybrid();
    let mut resp = Responder::new(
        &pure,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]),
        &mut srng,
    )
    .allow_non_hybrid();
    let (c, _) = mutual_handshake(&mut init, &mut resp).unwrap();
    assert!(!c.suite().is_hybrid());
}

// ---- MG-SEC-13 -------------------------------------------------------------

#[test]
fn a_hostile_count_is_refused_without_allocating() {
    let (ca, client, server) = fixtures();
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
    let mut bytes = encode_message(&HandshakeMessage::ClientHello(init.start()));
    // tag, version, 32-byte nonce, then the u32 KEM-suite count.
    bytes[34..38].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        decode_message(&bytes),
        Err(TransportError::Malformed(_))
    ));
}

#[test]
fn a_frame_larger_than_the_limit_is_refused_before_it_is_read() {
    let mut big = (1u32 << 30).to_le_bytes().to_vec();
    big.extend_from_slice(&[0; 16]);
    assert!(matches!(
        read_frame_limited(&mut Cursor::new(big), 64 << 10),
        Err(TransportError::Malformed(_))
    ));
}

#[test]
fn a_frame_that_announces_more_than_it_sends_is_an_error() {
    // Announces 60 KiB (inside the limit), sends 10 bytes, then the connection ends.
    let mut short = (60u32 << 10).to_le_bytes().to_vec();
    short.extend_from_slice(&[7; 10]);
    assert!(matches!(
        read_frame_limited(&mut Cursor::new(short), 64 << 10),
        Err(TransportError::Io(_))
    ));
}
