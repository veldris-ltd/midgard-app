//! The delta-transport seam. Picture-sync exchanges [`SignedDelta`]s over a
//! [`DeltaLink`]; in deployment the link is a `pq-mtls` session, in tests an
//! in-process [`LoopbackLink`]. This mirrors how `pq-mtls` itself models its byte
//! transport as an injected channel. [FR-H-6]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::error::SyncError;
use crate::replicator::SignedDelta;

/// A bidirectional link carrying signed deltas between two nodes.
pub trait DeltaLink {
    /// Send a signed delta to the peer.
    fn send(&mut self, delta: SignedDelta) -> Result<(), SyncError>;
    /// Receive the next signed delta from the peer, if one is queued.
    fn recv(&mut self) -> Result<SignedDelta, SyncError>;
}

/// An in-process link endpoint (single-threaded), standing in for a `pq-mtls`
/// session in tests.
pub struct LoopbackLink {
    outbox: Rc<RefCell<VecDeque<SignedDelta>>>,
    inbox: Rc<RefCell<VecDeque<SignedDelta>>>,
}

/// Create a connected pair of loopback endpoints (two nodes on one host).
pub fn loopback_link() -> (LoopbackLink, LoopbackLink) {
    let a: Rc<RefCell<VecDeque<SignedDelta>>> = Rc::new(RefCell::new(VecDeque::new()));
    let b: Rc<RefCell<VecDeque<SignedDelta>>> = Rc::new(RefCell::new(VecDeque::new()));
    (
        LoopbackLink {
            outbox: a.clone(),
            inbox: b.clone(),
        },
        LoopbackLink {
            outbox: b,
            inbox: a,
        },
    )
}

impl DeltaLink for LoopbackLink {
    fn send(&mut self, delta: SignedDelta) -> Result<(), SyncError> {
        self.outbox.borrow_mut().push_back(delta);
        Ok(())
    }
    fn recv(&mut self) -> Result<SignedDelta, SyncError> {
        self.inbox
            .borrow_mut()
            .pop_front()
            .ok_or(SyncError::LinkClosed)
    }
}
