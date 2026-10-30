//! Fail-closed negotiation (AC-M-1): no common PQ suite → the handshake aborts.
#![cfg(feature = "test-backend")]

mod common;

use policy::{ClassificationLevel, Purpose};
use pq_mtls::{mutual_handshake, Initiator, MtlsError, Posture, Responder};
use pqc_agility::{KemSuiteId, SeedRng};

#[test]
fn no_common_pq_suite_fails_closed() {
    let full = common::provider();
    // The client offers no KEM suites at all (models a classical-only / stripped peer).
    let mut empty = common::provider();
    for s in [
        KemSuiteId::MlKem768,
        KemSuiteId::MlKem1024,
        KemSuiteId::HybridX25519MlKem768,
        KemSuiteId::HybridX25519MlKem1024,
    ] {
        empty.disable_kem(s);
    }

    let ca = common::ca(1);
    let client = common::party("c", &["analyst"], 10, &ca);
    let server = common::party("s", &["server"], 20, &ca);
    let pdp = common::permit_pdp("analyst");
    let resource = common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]);

    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
    let mut init = Initiator::new(
        &empty,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        server.cert.identity.id.clone(),
        Purpose("investigation".into()),
        Posture::unclassified(),
        &mut crng,
    );
    let mut resp = Responder::new(
        &full,
        server.cert.clone(),
        &server.signing_key,
        ca.anchor.clone(),
        &pdp,
        resource,
        &mut srng,
    );

    let err = mutual_handshake(&mut init, &mut resp).unwrap_err();
    assert!(matches!(err, MtlsError::Negotiation(_)), "{err:?}");
}
