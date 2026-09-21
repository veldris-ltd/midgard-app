//! The loopback transport carries typed handshake messages between endpoints.

use pq_mtls::channel::{loopback_pair, Channel};
use pq_mtls::handshake::{Finished, HandshakeMessage};

#[test]
fn loopback_delivers_messages() {
    let (mut a, mut b) = loopback_pair();
    a.send(HandshakeMessage::Finished(Finished {
        finished_r: [7u8; 32],
    }))
    .unwrap();
    let got = b.recv().unwrap();
    match got {
        HandshakeMessage::Finished(f) => assert_eq!(f.finished_r, [7u8; 32]),
        other => panic!("unexpected message: {other:?}"),
    }
}

#[test]
fn empty_channel_reports_closed() {
    let (_a, mut b) = loopback_pair();
    assert!(b.recv().is_err());
}
