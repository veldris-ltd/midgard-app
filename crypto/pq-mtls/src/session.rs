//! The established secure session. Holds the negotiated suite, the authenticated
//! peer identity, and the (zeroizing) traffic secret. Bulk AEAD record protection
//! is provided by the [`crate::record`] module ([`crate::record::RecordSealer`] /
//! [`crate::record::RecordOpener`]); a completed session hands out a sealer/opener
//! pair via [`Session::records`]. [NFR-SEC-1]

use pqc_agility::{KemSuiteId, SecretArray};

use crate::identity::WorkloadIdentity;

/// A completed, mutually-authenticated session. `Debug` is derived (the traffic
/// secret redacts itself via `SecretArray`) so handshake results are usable with
/// `Result::unwrap`/`unwrap_err` in tests and callers.
#[derive(Debug)]
pub struct Session {
    peer: WorkloadIdentity,
    suite: KemSuiteId,
    traffic_secret: SecretArray<32>,
}

impl Session {
    pub(crate) fn new(peer: WorkloadIdentity, suite: KemSuiteId, traffic: [u8; 32]) -> Self {
        Self {
            peer,
            suite,
            traffic_secret: SecretArray::new(traffic),
        }
    }

    /// The authenticated peer identity.
    pub fn peer(&self) -> &WorkloadIdentity {
        &self.peer
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
    pub fn resumption_secret(&self) -> [u8; 32] {
        pqc_agility::hash::hkdf_sha256_32(
            b"midgard-pqmtls-resumption-master-v1",
            self.traffic_secret.expose(),
            b"resumption-master",
        )
    }
}
