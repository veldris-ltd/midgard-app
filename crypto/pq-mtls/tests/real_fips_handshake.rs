//! End-to-end PQ-mTLS handshake over the **production FIPS backend** — the real
//! hybrid X25519+ML-KEM-768 key agreement with real ML-DSA identity signatures.
//! This lifts AC-M-1 and AC-M-3 from "proven with the insecure test backend" to
//! "proven with production cryptography". [AC-M-1] [AC-M-3] [NFR-PQ-1] [NFR-SEC-1]
#![cfg(feature = "backend-fips")]

mod fips_fixtures;
use fips_fixtures::{
    ca, channel_resource, deny_pdp, party, permit_pdp, posture, provider, SeedRng,
};

use policy::Purpose;
use pq_mtls::{mutual_handshake, Initiator, MtlsError, Responder, Role as RecordRole, ServiceId};
use pqc_agility::KemSuiteId;

/// AC-M-1 (happy path): a real handshake negotiates the hybrid X25519+ML-KEM-768
/// suite and both sides derive the same traffic secret over real lattice KEM.
/// AC-M-3 (happy path): mutual authentication succeeds and each side ends up
/// holding the other's authenticated identity.
#[test]
fn real_handshake_negotiates_hybrid_and_agrees() {
    let provider = provider();
    let ca = ca(&provider, 1);
    let client = party(&provider, "svc-client", &["analyst"], 10, &ca);
    let server = party(&provider, "svc-server", &["server"], 20, &ca);
    let pdp = permit_pdp("analyst");

    let mut crng = SeedRng(1000);
    let mut srng = SeedRng(2000);
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

    let (client_session, server_session) =
        mutual_handshake(&mut init, &mut resp).expect("real-crypto handshake must succeed");

    // AC-M-1: the negotiated suite is the hybrid X25519+ML-KEM-768 default.
    assert_eq!(client_session.suite(), KemSuiteId::HybridX25519MlKem768);
    assert_eq!(server_session.suite(), KemSuiteId::HybridX25519MlKem768);

    // Both sides derived the same traffic secret (real KEM agreement).
    assert_eq!(
        client_session.traffic_secret(),
        server_session.traffic_secret(),
        "client and server must agree on the traffic secret"
    );

    // AC-M-3: each side authenticated the other's identity.
    assert_eq!(server_session.peer().id, ServiceId("svc-client".into()));
    assert_eq!(client_session.peer().id, ServiceId("svc-server".into()));
}

/// After a real handshake, the AES-256-GCM record layer carries application
/// bytes in both directions, and replay / tamper fail closed.
#[test]
fn real_record_layer_roundtrips_and_fails_closed() {
    let provider = provider();
    let ca = ca(&provider, 100);
    let client = party(&provider, "svc-client", &["analyst"], 110, &ca);
    let server = party(&provider, "svc-server", &["server"], 120, &ca);
    let pdp = permit_pdp("analyst");

    let mut crng = SeedRng(9001);
    let mut srng = SeedRng(9002);
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

    let (mut c_seal, mut c_open) = cs.records(RecordRole::Initiator);
    let (mut s_seal, mut s_open) = ss.records(RecordRole::Responder);

    // Client -> server.
    let req = b"GET /objects/42";
    let rec = c_seal.seal(b"h1", req).unwrap();
    assert_eq!(s_open.open(b"h1", &rec).unwrap(), req);

    // Server -> client.
    let resp_msg = b"200 OK {\"id\":42}";
    let rec2 = s_seal.seal(b"h1", resp_msg).unwrap();
    assert_eq!(c_open.open(b"h1", &rec2).unwrap(), resp_msg);

    // Replay of the first record (already consumed) fails: opener has advanced,
    // so the sequence-derived nonce no longer matches.
    assert!(s_open.open(b"h1", &rec).is_err(), "replay must fail closed");

    // Tamper fails closed.
    let mut bad = c_seal.seal(b"h1", b"more data").unwrap();
    bad[0] ^= 0x01;
    assert!(s_open.open(b"h1", &bad).is_err(), "tamper must fail closed");
}

/// AC-M-1 (fail-closed): when the two sides share no common post-quantum suite,
/// the handshake aborts — there is no downgrade to a weaker/classical suite.
#[test]
fn real_handshake_fails_closed_without_common_pq_suite() {
    // Client offers only hybrid-768; server offers only hybrid-1024 → no overlap.
    let mut client_provider = provider();
    client_provider.set_kem_preference(vec![KemSuiteId::HybridX25519MlKem768]);
    for s in [
        KemSuiteId::MlKem768,
        KemSuiteId::MlKem1024,
        KemSuiteId::HybridX25519MlKem1024,
    ] {
        client_provider.disable_kem(s);
    }
    let mut server_provider = provider();
    server_provider.set_kem_preference(vec![KemSuiteId::HybridX25519MlKem1024]);
    for s in [
        KemSuiteId::MlKem768,
        KemSuiteId::MlKem1024,
        KemSuiteId::HybridX25519MlKem768,
    ] {
        server_provider.disable_kem(s);
    }

    // Identities still come from a fully-capable provider so signing works.
    let idp = provider();
    let ca = ca(&idp, 3);
    let client = party(&idp, "c", &["analyst"], 30, &ca);
    let server = party(&idp, "s", &["server"], 40, &ca);
    let pdp = permit_pdp("analyst");

    let mut crng = SeedRng(11);
    let mut srng = SeedRng(22);
    let mut init = Initiator::new(
        &client_provider,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        server.cert.identity.id.clone(),
        Purpose("investigation".into()),
        posture(),
        &mut crng,
    );
    let mut resp = Responder::new(
        &server_provider,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        channel_resource(),
        &mut srng,
    );

    let err = mutual_handshake(&mut init, &mut resp).unwrap_err();
    assert!(
        matches!(err, MtlsError::Negotiation(_)),
        "expected fail-closed negotiation error, got {err:?}"
    );
}

/// AC-M-3 (fail-closed): an authenticated peer that policy does not permit is
/// denied — mutual authentication is necessary but not sufficient.
#[test]
fn real_handshake_denies_unauthorized_peer() {
    let provider = provider();
    let ca = ca(&provider, 5);
    let client = party(&provider, "svc-client", &["analyst"], 50, &ca);
    let server = party(&provider, "svc-server", &["server"], 60, &ca);
    let pdp = deny_pdp(); // deny-by-default: no rule permits OpenChannel

    let mut crng = SeedRng(5);
    let mut srng = SeedRng(6);
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

    let err = mutual_handshake(&mut init, &mut resp).unwrap_err();
    assert!(
        matches!(err, MtlsError::Authz(_)),
        "expected authz denial, got {err:?}"
    );
}

/// AC-M-3 (fail-closed): a certificate that does not chain to the trusted CA is
/// rejected before any session is established.
#[test]
fn real_handshake_rejects_untrusted_cert() {
    let provider = provider();
    let ca_a = ca(&provider, 7);
    let ca_b = ca(&provider, 8);
    let client = party(&provider, "c", &["analyst"], 70, &ca_a);
    let server = party(&provider, "s", &["server"], 80, &ca_a);
    let pdp = permit_pdp("analyst");

    let mut crng = SeedRng(7);
    let mut srng = SeedRng(8);
    // Client trusts CA B, but the server's cert is signed by CA A.
    let mut init = Initiator::new(
        &provider,
        client.cert.clone(),
        &client.signing_key,
        ca_b.anchor.clone(),
        server.cert.identity.id.clone(),
        Purpose("investigation".into()),
        posture(),
        &mut crng,
    );
    let mut resp = Responder::new(
        &provider,
        server.cert.clone(),
        &server.signing_key,
        ca_a.anchor.clone(),
        &pdp,
        channel_resource(),
        &mut srng,
    );

    let err = mutual_handshake(&mut init, &mut resp).unwrap_err();
    assert!(
        matches!(err, MtlsError::PeerCertUntrusted),
        "expected untrusted-cert, got {err:?}"
    );
}
