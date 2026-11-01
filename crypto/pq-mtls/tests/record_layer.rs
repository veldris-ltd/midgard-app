//! The version-2 record layer over a real FIPS handshake: keys ratchet forward after
//! a set amount of traffic on both ends in lock-step, and the end of a stream is an
//! explicit, authenticated marker rather than the connection simply ending.
//! [MG-SEC-12]
#![cfg(feature = "backend-fips")]

mod fips_fixtures;
use fips_fixtures::{ca, channel_resource, party, permit_pdp, posture, provider, SeedRng};

use policy::Purpose;
use pq_mtls::{
    mutual_handshake, Initiator, MtlsError, Record, RekeyLimits, Responder, Role, Session,
};

fn sessions() -> (Session, Session) {
    let provider = provider();
    let ca = ca(&provider, 300);
    let client = party(&provider, "svc-client", &["analyst"], 310, &ca);
    let server = party(&provider, "svc-server", &["server"], 320, &ca);
    let pdp = permit_pdp("analyst");
    let mut crng = SeedRng(1);
    let mut srng = SeedRng(2);
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
    mutual_handshake(&mut init, &mut resp).unwrap()
}

#[test]
fn both_ends_rekey_in_lock_step_by_record_count() {
    let (cs, ss) = sessions();
    let limits = RekeyLimits {
        records: 3,
        bytes: u64::MAX,
    };
    let (mut seal, _) = cs.records_with_limits(Role::Initiator, limits);
    let (_, mut open) = ss.records_with_limits(Role::Responder, limits);
    let mut first_epoch_record = None;
    for i in 0..10u8 {
        let rec = seal.seal(b"aad", &[i; 17]).unwrap();
        if i == 0 {
            first_epoch_record = Some(rec.clone());
        }
        assert_eq!(open.open(b"aad", &rec).unwrap(), vec![i; 17]);
    }
    assert_eq!(seal.epoch(), 3);
    assert_eq!(open.epoch(), 3);
    // A record from the first key is useless once both ends have moved on.
    assert!(open.open(b"aad", &first_epoch_record.unwrap()).is_err());
}

#[test]
fn both_ends_rekey_by_volume() {
    let (cs, ss) = sessions();
    let limits = RekeyLimits {
        records: u64::MAX,
        bytes: 1000,
    };
    let (mut seal, _) = cs.records_with_limits(Role::Initiator, limits);
    let (_, mut open) = ss.records_with_limits(Role::Responder, limits);
    for _ in 0..5 {
        let rec = seal.seal(b"aad", &[0u8; 600]).unwrap();
        open.open(b"aad", &rec).unwrap();
    }
    assert!(seal.epoch() >= 2);
    assert_eq!(seal.epoch(), open.epoch());
}

#[test]
fn ends_with_different_limits_do_not_interoperate_past_the_first_rekey() {
    let (cs, ss) = sessions();
    let (mut seal, _) = cs.records_with_limits(
        Role::Initiator,
        RekeyLimits {
            records: 2,
            bytes: u64::MAX,
        },
    );
    let (_, mut open) = ss.records(Role::Responder);
    open.open(b"aad", &seal.seal(b"aad", b"one").unwrap())
        .unwrap();
    open.open(b"aad", &seal.seal(b"aad", b"two").unwrap())
        .unwrap();
    assert!(open
        .open(b"aad", &seal.seal(b"aad", b"three").unwrap())
        .is_err());
}

#[test]
fn the_end_of_stream_marker_is_explicit_and_final() {
    let (cs, ss) = sessions();
    let (mut c_seal, _) = cs.records(Role::Initiator);
    let (_, mut s_open) = ss.records(Role::Responder);

    let data = c_seal.seal(b"aad", b"last request").unwrap();
    let close = c_seal.seal_close(b"aad").unwrap();
    assert_eq!(
        s_open.open_record(b"aad", &data).unwrap(),
        Record::Data(b"last request".to_vec())
    );
    assert!(!s_open.is_closed());
    assert_eq!(s_open.open_record(b"aad", &close).unwrap(), Record::Close);
    assert!(s_open.is_closed());

    // Nothing is sealed or accepted after the marker.
    assert!(matches!(
        c_seal.seal(b"aad", b"more"),
        Err(MtlsError::StreamClosed)
    ));
    assert!(s_open.open_record(b"aad", &data).is_err());
}

#[test]
fn open_reports_the_marker_where_data_was_expected() {
    let (cs, ss) = sessions();
    let (mut c_seal, _) = cs.records(Role::Initiator);
    let (_, mut s_open) = ss.records(Role::Responder);
    let close = c_seal.seal_close(b"aad").unwrap();
    assert!(matches!(
        s_open.open(b"aad", &close),
        Err(MtlsError::StreamClosed)
    ));
}

#[test]
fn a_forged_marker_does_not_authenticate() {
    let (cs, ss) = sessions();
    let (mut c_seal, _) = cs.records(Role::Initiator);
    let (_, mut s_open) = ss.records(Role::Responder);
    let mut close = c_seal.seal_close(b"aad").unwrap();
    close[0] ^= 1;
    assert!(s_open.open_record(b"aad", &close).is_err());
    assert!(!s_open.is_closed());
}
