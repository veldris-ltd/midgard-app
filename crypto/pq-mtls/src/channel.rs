//! The message transport seam. The handshake exchanges typed
//! [`HandshakeMessage`]s over a [`Channel`]; byte framing over a real socket
//! (length-prefixed, reusing the canonical encoders) is provided by
//! [`crate::wire::TcpChannel`], and an in-process [`LoopbackChannel`] backs tests
//! and single-node transports. [NFR-SEC-1]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::error::TransportError;
use crate::handshake::HandshakeMessage;

/// A bidirectional channel carrying handshake messages.
pub trait Channel {
    /// Send a message to the peer.
    fn send(&mut self, msg: HandshakeMessage) -> Result<(), TransportError>;
    /// Receive the next message from the peer.
    fn recv(&mut self) -> Result<HandshakeMessage, TransportError>;
}

/// An in-process channel endpoint (single-threaded).
pub struct LoopbackChannel {
    outbox: Rc<RefCell<VecDeque<HandshakeMessage>>>,
    inbox: Rc<RefCell<VecDeque<HandshakeMessage>>>,
}

/// Create a connected pair of loopback endpoints.
pub fn loopback_pair() -> (LoopbackChannel, LoopbackChannel) {
    let a: Rc<RefCell<VecDeque<HandshakeMessage>>> = Rc::new(RefCell::new(VecDeque::new()));
    let b: Rc<RefCell<VecDeque<HandshakeMessage>>> = Rc::new(RefCell::new(VecDeque::new()));
    (
        LoopbackChannel {
            outbox: a.clone(),
            inbox: b.clone(),
        },
        LoopbackChannel {
            outbox: b,
            inbox: a,
        },
    )
}

impl Channel for LoopbackChannel {
    fn send(&mut self, msg: HandshakeMessage) -> Result<(), TransportError> {
        self.outbox.borrow_mut().push_back(msg);
        Ok(())
    }
    fn recv(&mut self) -> Result<HandshakeMessage, TransportError> {
        self.inbox
            .borrow_mut()
            .pop_front()
            .ok_or(TransportError::Closed)
    }
}
