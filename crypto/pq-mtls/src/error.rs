//! Handshake and transport errors. Every [`MtlsError`] variant is a denial —
//! there is no "Ok that skipped a check".

use core::fmt;
use pqc_agility::{CryptoError, NegotiationError};

/// Zero-trust handshake failures. All are fail-closed. [NFR-SEC-1] [AC-M-1] [AC-M-3]
#[derive(Debug)]
#[non_exhaustive]
pub enum MtlsError {
    /// No common post-quantum suite could be negotiated.
    Negotiation(NegotiationError),
    /// The required crypto backend was unavailable (never a classical fallback).
    BackendUnavailable,
    /// The peer's certificate did not chain to a trust anchor.
    PeerCertUntrusted,
    /// The peer's certificate is on the revocation list (CRL). [RC-35]
    PeerCertRevoked,
    /// The peer's transcript signature failed verification.
    PeerSignatureInvalid,
    /// The peer presented no credential (mutual auth is mandatory).
    MissingPeerCredential,
    /// Key confirmation (finished MAC) failed — includes tampered ciphertext.
    KeyConfirmationFailed,
    /// The negotiated suite/capabilities were downgraded on the wire.
    DowngradeDetected,
    /// Authorization was denied after key confirmation.
    Authz(AuthzError),
    /// The transport failed.
    Transport(TransportError),
    /// A protocol invariant was violated (e.g. step called out of order).
    Protocol(String),
    /// A cryptographic operation failed.
    Crypto(CryptoError),
}

impl fmt::Display for MtlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MtlsError::Negotiation(e) => write!(f, "negotiation failed: {e}"),
            MtlsError::BackendUnavailable => write!(f, "crypto backend unavailable"),
            MtlsError::PeerCertUntrusted => write!(f, "peer certificate not trusted"),
            MtlsError::PeerCertRevoked => write!(f, "peer certificate is revoked"),
            MtlsError::PeerSignatureInvalid => write!(f, "peer signature invalid"),
            MtlsError::MissingPeerCredential => write!(f, "peer presented no credential"),
            MtlsError::KeyConfirmationFailed => write!(f, "key confirmation failed"),
            MtlsError::DowngradeDetected => write!(f, "downgrade detected"),
            MtlsError::Authz(e) => write!(f, "authorization denied: {e}"),
            MtlsError::Transport(e) => write!(f, "transport error: {e}"),
            MtlsError::Protocol(s) => write!(f, "protocol error: {s}"),
            MtlsError::Crypto(e) => write!(f, "crypto error: {e}"),
        }
    }
}

impl std::error::Error for MtlsError {}

/// Authorization outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthzError {
    /// The policy decision point denied the request.
    Denied,
}

impl fmt::Display for AuthzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthzError::Denied => write!(f, "denied by policy"),
        }
    }
}

impl std::error::Error for AuthzError {}

/// Transport-layer errors.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransportError {
    /// The channel was closed / empty.
    Closed,
    /// An underlying I/O error (socket read/write).
    Io(String),
    /// A frame could not be decoded (truncated, over-long, or unknown tag).
    Malformed(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Closed => write!(f, "channel closed"),
            TransportError::Io(s) => write!(f, "transport io: {s}"),
            TransportError::Malformed(s) => write!(f, "malformed frame: {s}"),
        }
    }
}

impl std::error::Error for TransportError {}
