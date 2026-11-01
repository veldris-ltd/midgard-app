//! # midgard-client-enclosure — the PQ-mTLS client to the enclosure
//!
//! All client-to-enclosure traffic is a **hybrid post-quantum mTLS** channel, and
//! it **fails closed**: if there is no common post-quantum suite, or the server is
//! not the pinned enclosure, or the transcript signature or key confirmation does
//! not check, no [`Session`] is produced. `[FR-C-5]` `[NFR-PQ-1]` `[AC-C-3]`
//!
//! There is no new crypto here — that is the point of KDD-1. The client is simply
//! the [`Initiator`] side of the suite-wide handshake (`crypto/pq-mtls`), which
//! already refuses a classical-only or downgraded peer. This crate adds the
//! client-shaped ergonomics: the **enclosure trust anchor is the certificate pin**,
//! and the **gateway identity is pinned too**: a server holding a valid certificate
//! for any other service is refused before the client sends its key exchange or its
//! own certificate. [MG-SEC-01]
//!
//! The transport is a [`Channel`], so the same client runs over a TCP socket
//! (native shells) or any other framed transport (a WASM bridge) without change.

#![forbid(unsafe_code)]

use policy::Purpose;
use pq_mtls::{
    run_initiator, Channel, IdentityCert, Initiator, MtlsError, Posture, ServiceId, Session,
    TrustAnchor,
};
use pqc_agility::{CryptoProvider, Rng, SigningKey};

mod error;
pub use error::EnclosureError;

/// A client that opens hybrid PQ-mTLS sessions to the enclosure gateway.
///
/// Holds the client's own certificate and the connection's declared purpose and
/// posture, plus the **pins**: the enclosure trust anchor (which certificate chain
/// to trust) and the exact gateway [`ServiceId`] to accept.
pub struct EnclosureClient {
    cert: IdentityCert,
    trust_anchor: TrustAnchor,
    gateway: ServiceId,
    purpose: Purpose,
    posture: Posture,
}

impl EnclosureClient {
    /// Build a client. `trust_anchor` is the certificate pin — only servers whose
    /// cert chains to it are trusted — and `gateway` the identity pin: only that
    /// service is accepted. [MG-SEC-01]
    pub fn new(
        cert: IdentityCert,
        trust_anchor: TrustAnchor,
        gateway: ServiceId,
        purpose: Purpose,
        posture: Posture,
    ) -> Self {
        Self {
            cert,
            trust_anchor,
            gateway,
            purpose,
            posture,
        }
    }

    /// Pin a different gateway identity than the one given to [`EnclosureClient::new`].
    pub fn pin_peer(mut self, peer: ServiceId) -> Self {
        self.gateway = peer;
        self
    }

    /// Open a session over `channel`. `provider`, `signing_key`, and `rng` are
    /// borrowed for the duration of the handshake. Fails closed on any check;
    /// on success the session is guaranteed post-quantum and, if a peer is pinned,
    /// to be exactly that peer. `[AC-C-3]`
    pub fn connect<'a>(
        &self,
        provider: &'a CryptoProvider,
        signing_key: &'a SigningKey,
        rng: &'a mut dyn Rng,
        channel: &mut dyn Channel,
    ) -> Result<Session, EnclosureError> {
        let init = Initiator::new(
            provider,
            self.cert.clone(),
            signing_key,
            self.trust_anchor.clone(),
            self.gateway.clone(),
            self.purpose.clone(),
            self.posture.clone(),
            rng,
        );
        let session = run_initiator(init, channel).map_err(|e| match e {
            MtlsError::PeerIdentityMismatch { expected, got } => EnclosureError::PeerPinMismatch {
                expected,
                actual: got,
            },
            e => EnclosureError::Handshake(e),
        })?;

        // Defence in depth: the Initiator already refuses a non-PQ suite, but the
        // client contract is explicit — a session is PQ or it is nothing.
        if !session.suite().is_post_quantum() {
            return Err(EnclosureError::Handshake(MtlsError::DowngradeDetected));
        }

        // Defence in depth: the Initiator already refused any other gateway.
        if session.peer().id != self.gateway {
            return Err(EnclosureError::PeerPinMismatch {
                expected: self.gateway.0.clone(),
                actual: session.peer().id.0.clone(),
            });
        }

        Ok(session)
    }
}
