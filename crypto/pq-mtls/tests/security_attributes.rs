//! Clearance and permitted purposes are certificate attributes the CA signs: a posture
//! above the certificate is clipped to it, a purpose outside it is refused, the
//! attributes cannot be edited on a copied certificate, and a certificate issued
//! before purposes were bound (version 2) still loads, verifies and connects,
//! unconstrained in purpose. Every handshake here crosses the wire codec, so the
//! version 3 encodings are exercised too. [VLD-ANB-MIDGARD-0063]
#![cfg(any(feature = "test-backend", feature = "backend-fips"))]

mod common;

use std::collections::BTreeSet;

use common::SeedRng;
use policy::{ClassificationLevel, Purpose};
use pq_mtls::{
    canonical_cert_body, certify, decode_cert, decode_message, encode_cert, encode_message,
    verify_cert, Authorization, AuthzError, CertTerms, HandshakeMessage, Initiator, MtlsError,
    Posture, Responder, ServiceId, CERT3_MAGIC, CERT_MAGIC,
};

const NOW: u64 = 1_800_000_000;
/// The policy lattice's third level: the "restricted" band.
const RESTRICTED: ClassificationLevel = ClassificationLevel::SECRET;

/// Terms granting `level`, binding `purposes` if given.
fn terms(level: ClassificationLevel, purposes: Option<&[&str]>) -> CertTerms {
    let t = CertTerms::new(1, 0, u64::MAX, Posture::at(level));
    match purposes {
        Some(p) => t.with_purposes(p.iter().map(|p| Purpose(p.to_string()))),
        None => t,
    }
}

/// An analyst certified under `terms`.
fn client(ca: &common::Ca, terms: CertTerms) -> common::Party {
    let mut p = common::party("analyst-svc", &["analyst"], 10, ca);
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

/// A message, encoded and decoded as it would cross a socket.
fn over_wire(m: HandshakeMessage) -> HandshakeMessage {
    decode_message(&encode_message(&m)).expect("the message decodes")
}

/// A full handshake through the wire codec: the client declares `purpose` and asks for
/// `requested`; the server's channel is marked `level` and allows `allowed`. Returns
/// what the responder authorised.
fn handshake(
    client: &common::Party,
    server: &common::Party,
    ca: &common::Ca,
    purpose: &str,
    requested: Posture,
    level: ClassificationLevel,
    allowed: &[&str],
) -> Result<Authorization, MtlsError> {
    let provider = common::provider();
    let pdp = common::permit_pdp("analyst");
    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
    let mut init = Initiator::new(
        &provider,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        ServiceId("gateway".into()),
        Purpose(purpose.into()),
        requested,
        &mut crng,
    )
    .at_time(NOW);
    let mut resp = Responder::new(
        &provider,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        common::channel_resource(level, allowed),
        &mut srng,
    )
    .at_time(NOW);

    let HandshakeMessage::ClientHello(ch) = over_wire(HandshakeMessage::ClientHello(init.start()))
    else {
        unreachable!()
    };
    let sh = resp.on_client_hello(ch)?;
    let HandshakeMessage::ServerHello(sh) = over_wire(HandshakeMessage::ServerHello(sh)) else {
        unreachable!()
    };
    let cke = init.on_server_hello(sh)?;
    let HandshakeMessage::ClientKeyExchange(cke) =
        over_wire(HandshakeMessage::ClientKeyExchange(cke))
    else {
        unreachable!()
    };
    let (fin, session) = resp.on_client_key_exchange(cke)?;
    let HandshakeMessage::Finished(fin) = over_wire(HandshakeMessage::Finished(fin)) else {
        unreachable!()
    };
    init.on_finished(fin)?;
    Ok(session
        .authorization()
        .cloned()
        .expect("responder authorises"))
}

#[test]
fn a_posture_above_the_certificate_is_clipped_to_the_certificate() {
    // [VLD-ANB-MIDGARD-0063] [AC-22.1]
    let ca = common::ca(1);
    let client = client(
        &ca,
        terms(ClassificationLevel::UNCLASSIFIED, Some(&["investigation"])),
    );
    let server = common::party("gateway", &["server"], 20, &ca);
    let top = Posture::at(ClassificationLevel::TOP_SECRET);

    // Declaring the top band does not reach a restricted channel: the certificate
    // grants open.
    let refused = handshake(
        &client,
        &server,
        &ca,
        "investigation",
        top.clone(),
        RESTRICTED,
        &["investigation"],
    );
    assert!(
        matches!(refused, Err(MtlsError::Authz(AuthzError::Denied))),
        "{refused:?}"
    );

    // The same client reaches an open channel, authorised at the certificate's band.
    let auth = handshake(
        &client,
        &server,
        &ca,
        "investigation",
        top,
        ClassificationLevel::UNCLASSIFIED,
        &["investigation"],
    )
    .unwrap();
    assert_eq!(auth.clearance, Posture::unclassified());
}

#[test]
fn a_purpose_outside_the_certificate_is_refused() {
    // [VLD-ANB-MIDGARD-0063] [AC-22.2]
    let ca = common::ca(1);
    let client = client(
        &ca,
        terms(
            ClassificationLevel::TOP_SECRET,
            Some(&["situational-awareness"]),
        ),
    );
    let server = common::party("gateway", &["server"], 20, &ca);
    // The channel's policy allows both purposes; only the certificate stands in the way.
    let allowed = ["situational-awareness", "export"];

    let refused = handshake(
        &client,
        &server,
        &ca,
        "export",
        Posture::unclassified(),
        ClassificationLevel::UNCLASSIFIED,
        &allowed,
    );
    assert!(
        matches!(
            refused,
            Err(MtlsError::Authz(AuthzError::PurposeNotCertified))
        ),
        "{refused:?}"
    );

    let auth = handshake(
        &client,
        &server,
        &ca,
        "situational-awareness",
        Posture::unclassified(),
        ClassificationLevel::UNCLASSIFIED,
        &allowed,
    )
    .unwrap();
    assert_eq!(auth.purpose, Purpose("situational-awareness".into()));
    assert_eq!(
        auth.purposes,
        Some(BTreeSet::from([Purpose("situational-awareness".into())]))
    );
}

#[test]
fn a_v1_certificate_still_verifies_and_is_unconstrained() {
    // [VLD-ANB-MIDGARD-0063] [AC-22.3] On this branch the certificate before purposes
    // is version 2 (MGCERT2); it is written exactly as before and stays valid.
    let ca = common::ca(1);
    let legacy = client(&ca, terms(ClassificationLevel::TOP_SECRET, None));
    let bytes = encode_cert(&legacy.cert);
    assert!(bytes.starts_with(CERT_MAGIC));
    assert!(
        canonical_cert_body(&legacy.cert.identity, &legacy.cert.terms)
            .starts_with(b"midgard-pqmtls-cert-v2")
    );

    let decoded = decode_cert(&bytes).unwrap();
    assert_eq!(decoded.terms.purposes, None);
    verify_cert(&decoded, &ca.anchor, &common::provider(), NOW).unwrap();

    // It connects for any purpose the channel allows, and the session says it binds
    // none.
    let mut client = legacy;
    client.cert = decoded;
    let server = common::party("gateway", &["server"], 20, &ca);
    let auth = handshake(
        &client,
        &server,
        &ca,
        "export",
        Posture::unclassified(),
        ClassificationLevel::UNCLASSIFIED,
        &["export"],
    )
    .unwrap();
    assert_eq!(auth.purposes, None);

    // A version 2 certificate still travels under the version 2 ServerHello tag.
    let v3 = client_cert_v3(&ca);
    assert_ne!(
        server_hello_tag(&client.cert),
        server_hello_tag(&v3),
        "a version 3 certificate is framed apart from a version 2 one"
    );
    assert_eq!(server_hello_tag(&client.cert), 2);
}

fn client_cert_v3(ca: &common::Ca) -> pq_mtls::IdentityCert {
    client(
        ca,
        terms(ClassificationLevel::SECRET, Some(&["investigation"])),
    )
    .cert
}

/// The tag byte of a ServerHello presenting `cert`.
fn server_hello_tag(cert: &pq_mtls::IdentityCert) -> u8 {
    let ca = common::ca(1);
    let server = common::party("gateway", &["server"], 20, &ca);
    let provider = common::provider();
    let pdp = common::permit_pdp("analyst");
    let mut srng = SeedRng::new(2);
    let mut crng = SeedRng::new(1);
    let mut resp = Responder::new(
        &provider,
        cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]),
        &mut srng,
    )
    .at_time(NOW);
    let mut init = Initiator::new(
        &provider,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        ServiceId("gateway".into()),
        Purpose("investigation".into()),
        Posture::unclassified(),
        &mut crng,
    );
    let sh = resp.on_client_hello(init.start()).unwrap();
    encode_message(&HandshakeMessage::ServerHello(sh))[0]
}

#[test]
fn certificate_attributes_are_signed() {
    // [VLD-ANB-MIDGARD-0063] [AC-22.3]
    let ca = common::ca(1);
    let cert = client_cert_v3(&ca);
    let provider = common::provider();
    let bytes = encode_cert(&cert);
    assert!(bytes.starts_with(CERT3_MAGIC));
    let round = decode_cert(&bytes).unwrap();
    assert_eq!(round, cert);
    verify_cert(&round, &ca.anchor, &provider, NOW).unwrap();

    // Find the level byte: the one byte where an encoding at another level differs.
    let mut other = cert.clone();
    other.terms.clearance.level = ClassificationLevel::TOP_SECRET;
    let other_bytes = encode_cert(&other);
    let diffs: Vec<usize> = (0..bytes.len())
        .filter(|&i| bytes[i] != other_bytes[i])
        .collect();
    assert_eq!(diffs.len(), 1);
    let mut flipped = bytes.clone();
    flipped[diffs[0]] = ClassificationLevel::TOP_SECRET.0;
    let tampered = decode_cert(&flipped).unwrap();
    assert_eq!(
        tampered.terms.clearance.level,
        ClassificationLevel::TOP_SECRET
    );
    assert!(matches!(
        verify_cert(&tampered, &ca.anchor, &provider, NOW),
        Err(MtlsError::PeerCertUntrusted)
    ));

    // Nor can purposes be widened, or stripped to pass as a version 2 certificate.
    let mut wider = cert.clone();
    wider
        .terms
        .purposes
        .as_mut()
        .unwrap()
        .insert(Purpose("export".into()));
    let mut stripped = cert.clone();
    stripped.terms.purposes = None;
    for forged in [wider, stripped] {
        let forged = decode_cert(&encode_cert(&forged)).unwrap();
        assert!(matches!(
            verify_cert(&forged, &ca.anchor, &provider, NOW),
            Err(MtlsError::PeerCertUntrusted)
        ));
    }
}
