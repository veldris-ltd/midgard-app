//! # pq-mtls — zero-trust mutual authentication for inter-service calls
//!
//! A post-quantum, KEM-based handshake that is **mutually authenticated** and
//! **fails closed** on every check: no common PQ suite, an untrusted or missing
//! peer certificate, a bad transcript signature, an on-path downgrade, a failed
//! key confirmation, or a denied authorization all abort with a distinct
//! [`MtlsError`]. There is no code path that reaches an established [`Session`]
//! having skipped a check. `[NFR-SEC-1]` `[AC-M-1]` `[AC-M-3]`
//!
//! Authorization delegates to the `policy` crate's
//! [`policy::PolicyDecisionPoint`] (this crate depends on `policy`, never the
//! reverse). The handshake is modelled as typed messages over an injected
//! [`channel::Channel`]; [`wire`] provides the length-prefixed byte framing over
//! real sockets ([`wire::TcpChannel`]) and [`record`] the post-handshake
//! AES-256-GCM record layer ([`RecordSealer`]/[`RecordOpener`], handed out by
//! [`session::Session::records`]).

pub mod channel;
pub mod error;
pub mod handshake;
pub mod identity;
pub mod posture;
#[cfg(feature = "backend-fips")]
pub mod record;
#[cfg(feature = "backend-fips")]
pub mod resumption;
pub mod session;
pub mod wire;

pub use channel::{loopback_pair, Channel, LoopbackChannel};
pub use error::{AuthzError, MtlsError, TransportError};
pub use handshake::{
    mutual_handshake, ClientHello, ClientKeyExchange, Finished, HandshakeMessage, Initiator,
    Responder, ServerHello,
};
pub use identity::{
    canonical_identity, cert_fingerprint, verify_cert, IdentityCert, ServiceId, TrustAnchor,
    WorkloadIdentity,
};
pub use posture::Posture;
#[cfg(feature = "backend-fips")]
pub use record::{RecordOpener, RecordSealer, Role};
#[cfg(feature = "backend-fips")]
pub use resumption::{
    issue_ticket, run_resume_initiator, run_resume_responder, ClientTicket, Resumed, TicketKey,
    MODE_FULL, MODE_RESUME,
};
pub use session::Session;
pub use wire::{
    decode_anchor, decode_cert, decode_message, encode_anchor, encode_cert, encode_message,
    read_frame, run_initiator, run_responder, write_frame, TcpChannel, ANCHOR_MAGIC, CERT_MAGIC,
};
