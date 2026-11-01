//! The established secure session. Holds the negotiated suite, the authenticated
//! peer identity and certificate facts, what the responder authorised the peer for,
//! and the (zeroizing) traffic secret. Bulk AEAD record protection is provided by the
//! [`crate::record`] module ([`crate::record::RecordSealer`] /
//! [`crate::record::RecordOpener`]); a completed session hands out a sealer/opener
//! pair via [`Session::records`]. [NFR-SEC-1]

use policy::Purpose;
use pqc_agility::{KemSuiteId, SecretArray};

use crate::identity::WorkloadIdentity;
use crate::posture::Posture;

/// What the responder authorised the peer for: the effective clearance (the lower of
/// what its certificate grants and what it asked for) and the declared purpose. A
/// resumption ticket carries exactly this, so a resumed connection is re-authorised
/// as the full handshake was. [MG-SEC-04]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    /// The clearance the peer was authorised at.
    pub clearance: Posture,
    /// The purpose the peer declared.
    pub purpose: Purpose,
}

/// The peer certificate facts a session keeps so resumption can re-check them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PeerCert {
    pub(crate) fingerprint: [u8; 32],
    pub(crate) not_after: u64,
}

/// A completed, mutually-authenticated session. `Debug` is derived (the traffic
/// secret redacts itself via `SecretArray`) so handshake results are usable with
/// `Result::unwrap`/`unwrap_err` in tests and callers.
#[derive(Debug)]
pub struct Session {
    peer: WorkloadIdentity,
    peer_cert: PeerCert,
    authorization: Option<Authorization>,
    suite: KemSuiteId,
    traffic_secret: SecretArray<32>,
}

impl Session {
    pub(crate) fn new(
        peer: WorkloadIdentity,
        peer_cert: PeerCert,
        authorization: Option<Authorization>,
        suite: KemSuiteId,
        traffic: SecretArray<32>,
    ) -> Self {
        Self {
            peer,
            peer_cert,
            authorization,
            suite,
            traffic_secret: traffic,
        }
    }

    /// The authenticated peer identity.
    pub fn peer(&self) -> &WorkloadIdentity {
        &self.peer
    }

    /// The fingerprint of the certificate the peer presented (its CRL identity).
    pub fn peer_fingerprint(&self) -> [u8; 32] {
        self.peer_cert.fingerprint
    }

    /// When the peer's certificate stops being valid (Unix seconds, exclusive).
    pub fn peer_not_after(&self) -> u64 {
        self.peer_cert.not_after
    }

    #[cfg_attr(not(feature = "backend-fips"), allow(dead_code))]
    pub(crate) fn peer_cert(&self) -> PeerCert {
        self.peer_cert
    }

    /// What the responder authorised the peer for. `Some` on the responder's session,
    /// `None` on the initiator's (the initiator authorises nothing).
    pub fn authorization(&self) -> Option<&Authorization> {
        self.authorization.as_ref()
    }

    /// The negotiated KEM suite.
    pub fn suite(&self) -> KemSuiteId {
        self.suite
    }

    /// The 32-byte traffic secret. [`Session::records`] derives the two directional
    /// AES-256-GCM keys from it internally; this accessor exposes it so tests can
    /// confirm both peers agree on the same secret.
    pub fn traffic_secret(&self) -> &[u8; 32] {
        self.traffic_secret.expose()
    }

    /// The resumption master secret (RMS) for this session: a one-way derivation
    /// of the traffic secret. A future abbreviated handshake resumes from this
    /// pre-shared secret instead of re-running KEM + signatures. Compromising the
    /// RMS does not reveal the traffic secret (HKDF is one-way). [RT-25]
    pub fn resumption_secret(&self) -> SecretArray<32> {
        SecretArray::new(pqc_agility::hash::hkdf_sha256_32(
            b"midgard-pqmtls-resumption-master-v2",
            self.traffic_secret.expose(),
            b"resumption-master",
        ))
    }
}
