//! Happy path: a full mutual handshake yields equal traffic secrets and correct
//! peer identities over a post-quantum suite.
#![cfg(feature = "test-backend")]

mod common;

use std::collections::BTreeSet;

use policy::{ClassificationLevel, Purpose};
use pq_mtls::{mutual_handshake, Initiator, Posture, Responder};
use pqc_agility::SeedRng;

#[test]
fn successful_mutual_handshake() {
    let provider = common::provider();
    let ca = common::ca(1);
    let client = common::party("svc-client", &["analyst"], 10, &ca);
    let server = common::party("svc-server", &["server"], 20, &ca);
    let pdp = common::permit_pdp("analyst");
    let resource = common::channel_resource(ClassificationLevel::UNCLASSIFIED, &["investigation"]);

    let mut crng = SeedRng::new(100);
    let mut srng = SeedRng::new(200);
    let mut init = Initiator::new(
        &provider,
        client.cert.clone(),
        &client.signing_key,
        ca.anchor.clone(),
        server.cert.identity.id.clone(),
        Purpose("investigation".into()),
        Posture {
            level: ClassificationLevel::SECRET,
            compartments: BTreeSet::new(),
        },
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

    let (cs, ss) = mutual_handshake(&mut init, &mut resp).expect("handshake must succeed");
    assert_eq!(cs.traffic_secret(), ss.traffic_secret());
    assert_eq!(cs.peer().id.0, "svc-server");
    assert_eq!(ss.peer().id.0, "svc-client");
    assert!(cs.suite().is_post_quantum());
}
