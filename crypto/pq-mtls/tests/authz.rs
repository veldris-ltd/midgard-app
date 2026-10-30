//! Post-handshake authorization via the policy PDP (AC-M-3).
#![cfg(feature = "test-backend")]

mod common;

use std::collections::BTreeSet;

use policy::{ClassificationLevel, Purpose};
use pq_mtls::{mutual_handshake, Initiator, MtlsError, Posture, Responder};
use pqc_agility::SeedRng;

fn posture(level: ClassificationLevel) -> Posture {
    Posture {
        level,
        compartments: BTreeSet::new(),
    }
}

#[test]
fn deny_by_default_pdp_blocks_channel() {
    let provider = common::provider();
    let ca = common::ca(1);
    let client = common::party("c", &["analyst"], 10, &ca);
    let server = common::party("s", &["server"], 20, &ca);
    let pdp = common::deny_pdp(); // empty policy → deny-by-default
    let resource = common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]);

    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
    let mut init = Initiator::new(
        &provider,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        server.cert.identity.id.clone(),
        Purpose("investigation".into()),
        posture(ClassificationLevel::SECRET),
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
    let err = mutual_handshake(&mut init, &mut resp).unwrap_err();
    assert!(matches!(err, MtlsError::Authz(_)), "{err:?}");
}

#[test]
fn insufficient_posture_blocks_channel() {
    let provider = common::provider();
    let ca = common::ca(1);
    let client = common::party("c", &["analyst"], 10, &ca);
    let server = common::party("s", &["server"], 20, &ca);
    let pdp = common::permit_pdp("analyst");
    // Channel requires SECRET, but the client posture is only UNCLASSIFIED.
    let resource = common::channel_resource(ClassificationLevel::SECRET, &["investigation"]);

    let mut crng = SeedRng::new(1);
    let mut srng = SeedRng::new(2);
    let mut init = Initiator::new(
        &provider,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        server.cert.identity.id.clone(),
        Purpose("investigation".into()),
        posture(ClassificationLevel::UNCLASSIFIED),
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
    let err = mutual_handshake(&mut init, &mut resp).unwrap_err();
    assert!(matches!(err, MtlsError::Authz(_)), "{err:?}");
}
