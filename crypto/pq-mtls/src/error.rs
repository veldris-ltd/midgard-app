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
    /// The peer's certificate is outside its validity window. [MG-SEC-02]
    PeerCertExpired {
        /// Start of the certificate's validity.
        not_before: u64,
        /// End of the certificate's validity (exclusive).
        not_after: u64,
        /// The time it was checked at.
        now: u64,
    },
    /// The peer holds a valid certificate, but for another service than the one the
    /// caller meant to reach. Refused before anything is sent. [MG-SEC-01]
    PeerIdentityMismatch {
        /// The service the caller asked for.
        expected: String,
        /// The service the certificate names.
        got: String,
    },
    /// A revocation list did not verify under the trust anchor. [MG-SEC-03]
    CrlUntrusted,
    /// A revocation list is older than the one already installed. [MG-SEC-03]
    CrlRollback {
        /// The installed version.
        installed: u64,
        /// The version offered.
        offered: u64,
    },
    /// The negotiated suite is not a hybrid (ML-KEM + X25519) suite, and this side
    /// requires one. [MG-SEC-12]
    HybridRequired,
    /// The peer speaks a handshake protocol version this side does not. [MG-SEC-09]
    UnsupportedVersion(u8),
    /// The record stream ended without the peer's end-of-stream marker: the
    /// connection was cut, not finished. [MG-SEC-12]
    StreamTruncated,
    /// The peer sent its end-of-stream marker where data was expected. [MG-SEC-12]
    StreamClosed,
    /// The server is at its connection limit. [MG-SEC-13]
    Overloaded,
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
            MtlsError::PeerCertExpired {
                not_before,
                not_after,
                now,
            } => write!(
                f,
                "peer certificate is valid over [{not_before}, {not_after}), not at {now}"
            ),
            MtlsError::PeerIdentityMismatch { expected, got } => {
                write!(f, "peer is '{got}', not the expected '{expected}'")
            }
            MtlsError::CrlUntrusted => write!(f, "revocation list is not signed by the CA"),
            MtlsError::CrlRollback { installed, offered } => write!(
                f,
                "revocation list version {offered} would replace newer version {installed}"
            ),
            MtlsError::HybridRequired => write!(f, "a hybrid key exchange is required"),
            MtlsError::UnsupportedVersion(v) => write!(f, "unsupported protocol version {v}"),
            MtlsError::StreamTruncated => {
                write!(f, "record stream ended without an end-of-stream marker")
            }
            MtlsError::StreamClosed => write!(f, "peer closed the record stream"),
            MtlsError::Overloaded => write!(f, "server is at its connection limit"),
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
