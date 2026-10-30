//! Mutual-authentication failures all fail closed (AC-M-3).
#![cfg(feature = "test-backend")]

mod common;

use std::collections::BTreeSet;

use policy::{ClassificationLevel, DefaultPdp, Purpose, Resource};
use pq_mtls::{mutual_handshake, Initiator, MtlsError, Posture, Responder};
use pqc_agility::{Ciphertext, CryptoProvider, SeedRng};

fn fixtures() -> (
    CryptoProvider,
    common::Ca,
    common::Party,
    common::Party,
    DefaultPdp,
    Resource,
) {
    let provider = common::provider();
    let ca = common::ca(1);
    let client = common::party("svc-client", &["analyst"], 10, &ca);
    let server = common::party("svc-server", &["server"], 20, &ca);
    let pdp = common::permit_pdp("analyst");
    let resource = common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]);
    (provider, ca, client, server, pdp, resource)
}

fn posture() -> Posture {
    Posture {
        level: ClassificationLevel::SECRET,
        compartments: BTreeSet::new(),
    }
}

#[test]
fn untrusted_server_cert_is_rejected() {
    let provider = common::provider();
    let ca_a = common::ca(1);
    let ca_b = common::ca(2);
    let client = common::party("c", &["analyst"], 10, &ca_a);
    let server = common::party("s", &["server"], 20, &ca_a);
    let pdp = common::permit_pdp("analyst");
    let resource = common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]);

    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
    // The initiator trusts CA B, but the server cert is signed by CA A.
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
        resource,
        &mut srng,
    );
    let err = mutual_handshake(&mut init, &mut resp).unwrap_err();
    assert!(matches!(err, MtlsError::PeerCertUntrusted), "{err:?}");
}

#[test]
fn missing_client_credential_is_rejected() {
    let (provider, ca, client, server, pdp, resource) = fixtures();
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
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
        resource,
        &mut srng,
    );

    let ch = init.start();
    let sh = resp.on_client_hello(ch).unwrap();
    let mut cke = init.on_server_hello(sh).unwrap();
    cke.cert = None; // client omits its credential
    let err = resp.on_client_key_exchange(cke).unwrap_err();
    assert!(matches!(err, MtlsError::MissingPeerCredential), "{err:?}");
}

#[test]
fn on_path_downgrade_breaks_authentication() {
    let (provider, ca, client, server, pdp, resource) = fixtures();
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
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
        resource,
        &mut srng,
    );

    let ch = init.start();

    // Attacker strips the hybrid suites from the ClientHello in flight: with hybrid
    // required by default [MG-SEC-12] the server has nothing it will accept.
    let mut stripped = ch.clone();
    stripped.capabilities.kem_suites.retain(|s| !s.is_hybrid());
    assert!(resp.on_client_hello(stripped).is_err());

    // A subtler edit — dropping the preferred suite so another is chosen — gets past
    // the server, but the initiator's transcript still reflects the original
    // capabilities, so the server's signature no longer verifies.
    let mut srng = SeedRng::new(2);
    let mut resp = Responder::new(
        &provider,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]),
        &mut srng,
    );
    let mut edited = ch;
    let preferred = edited.capabilities.kem_suites[0];
    edited.capabilities.kem_suites.retain(|s| *s != preferred);
    let sh = resp.on_client_hello(edited).unwrap();
    let err = init.on_server_hello(sh).unwrap_err();
    assert!(matches!(err, MtlsError::PeerSignatureInvalid), "{err:?}");
}

#[test]
fn tampered_ciphertext_is_rejected() {
    let (provider, ca, client, server, pdp, resource) = fixtures();
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
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
        resource,
        &mut srng,
    );

    let ch = init.start();
    let sh = resp.on_client_hello(ch).unwrap();
    let mut cke = init.on_server_hello(sh).unwrap();
    let mut bytes = cke.ct.as_bytes().to_vec();
    bytes[0] ^= 0x01;
    cke.ct = Ciphertext::new(cke.ct.suite(), bytes);
    // The client signature covers the ciphertext, so tampering breaks auth.
    let err = resp.on_client_key_exchange(cke).unwrap_err();
    assert!(matches!(err, MtlsError::PeerSignatureInvalid), "{err:?}");
}

#[test]
fn tampered_key_confirmation_is_rejected() {
    let (provider, ca, client, server, pdp, resource) = fixtures();
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
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
        resource,
        &mut srng,
    );

    let ch = init.start();
    let sh = resp.on_client_hello(ch).unwrap();
    let mut cke = init.on_server_hello(sh).unwrap();
    cke.finished_i[0] ^= 0x01; // not covered by the signature; caught by key confirmation
    let err = resp.on_client_key_exchange(cke).unwrap_err();
    assert!(matches!(err, MtlsError::KeyConfirmationFailed), "{err:?}");
}
