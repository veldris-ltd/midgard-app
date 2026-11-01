//! # pq-mtls — zero-trust mutual authentication for inter-service calls
//!
//! A post-quantum, KEM-based handshake that is **mutually authenticated** and
//! **fails closed** on every check: no common PQ suite, an untrusted, expired,
//! revoked or missing peer certificate, a certificate for another service than the
//! one the caller meant to reach, a bad transcript signature, an on-path downgrade, a
//! failed key confirmation, or a denied authorization all abort with a distinct
//! [`MtlsError`]. There is no code path that reaches an established [`Session`]
//! having skipped a check. `[NFR-SEC-1]` `[AC-M-1]` `[AC-M-3]`
//!
//! Authorization delegates to the `policy` crate's
//! [`policy::PolicyDecisionPoint`] (this crate depends on `policy`, never the
//! reverse). The handshake is modelled as typed messages over an injected
//! [`channel::Channel`]; [`wire`] provides the length-prefixed byte framing over
//! real sockets ([`wire::TcpChannel`]) and [`record`] the post-handshake
//! AES-256-GCM record layer ([`RecordSealer`]/[`RecordOpener`], handed out by
//! [`session::Session::records`]). [`revocation`] carries the CA-signed revocation
//! list and reloads it live.

pub mod channel;
pub mod error;
pub mod handshake;
pub mod identity;
pub mod posture;
#[cfg(feature = "backend-fips")]
pub mod record;
#[cfg(feature = "backend-fips")]
pub mod resumption;
pub mod revocation;
pub mod session;
pub mod wire;

pub use channel::{loopback_pair, Channel, LoopbackChannel};
pub use error::{AuthzError, MtlsError, TransportError};
pub use handshake::{
    mutual_handshake, ClientHello, ClientKeyExchange, Finished, HandshakeMessage, Initiator,
    Responder, ServerHello, PROTOCOL_VERSION,
};
pub use identity::{
    canonical_cert_body, canonical_identity, cert_fingerprint, certify, effective_clearance,
    verify_cert, CertTerms, IdentityCert, ServiceId, TrustAnchor, WorkloadIdentity,
};
pub use posture::Posture;
#[cfg(feature = "backend-fips")]
pub use record::{Record, RecordOpener, RecordSealer, RekeyLimits, Role};
#[cfg(feature = "backend-fips")]
pub use resumption::{
    issue_ticket, run_resume_initiator, run_resume_responder, ClientTicket, Resumed, TicketKey,
    MODE_FULL, MODE_RESUME,
};
pub use revocation::{
    decode_crl, encode_crl, reload_crl_file, spawn_crl_reloader, RevocationList, CRL_MAGIC,
};
pub use session::{Authorization, Session};
pub use wire::{
    decode_anchor, decode_cert, decode_message, encode_anchor, encode_cert, encode_message,
    read_frame, read_frame_limited, read_handshake_frame, run_initiator, run_responder,
    write_frame, TcpChannel, ANCHOR_MAGIC, CERT_MAGIC, MAX_HANDSHAKE_FRAME, MAX_RECORD_FRAME,
};
