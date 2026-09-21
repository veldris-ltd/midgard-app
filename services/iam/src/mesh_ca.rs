//! The **mesh certificate authority**: operated issuance, renewal, and revocation of [NFR-SEC-2]
//! PQ-mTLS workload certificates on top of the IdP's ML-DSA CA key. Midgard uses
//! **short-lived, rekeyed certs + a CA-authoritative registry + a CRL distributed in
//! the trust anchor** (rather than long-lived certs with embedded validity):
//!
//! - **Issue** — certify a workload identity and record its validity window + serial.
//! - **Renew** — issue a *new* cert (new keypair) for the same subject with a fresh
//!   window. Because the old cert stays valid until it expires or is revoked, renewal
//!   causes **no downtime**: live sessions continue and new handshakes use the new
//!   cert. [RC-35]
//! - **Revoke** — place a cert's verifying-key fingerprint on the CRL; the CRL rides
//!   in [`TrustAnchor`], so a revoked cert **fails the next handshake**. [RC-35]
//!
//! The registry is the authority on what was issued and when; a short renewal cadence
//! keeps live certs fresh, and revocation is the immediate kill-switch.

use std::collections::{BTreeMap, BTreeSet};

use pq_mtls::{cert_fingerprint, IdentityCert, ServiceId, TrustAnchor, WorkloadIdentity};

use crate::idp::Idp;
use crate::token::TokenError;

/// The lifecycle state of an issued cert at a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertStatus {
    /// Within its validity window and not revoked.
    Valid,
    /// Not yet within its validity window.
    NotYetValid,
    /// Past its validity window.
    Expired,
    /// On the revocation list.
    Revoked,
    /// No such serial.
    Unknown,
}

/// A CA operation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaError {
    /// No cert with this serial was issued by this CA.
    UnknownSerial(u64),
    /// Signing the cert failed.
    Sign(TokenError),
}

impl std::fmt::Display for CaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaError::UnknownSerial(s) => write!(f, "unknown certificate serial {s}"),
            CaError::Sign(e) => write!(f, "certificate signing failed: {e}"),
        }
    }
}
impl std::error::Error for CaError {}

/// The CA's record of one issued certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedCert {
    /// Monotonic serial number.
    pub serial: u64,
    /// The certified subject.
    pub subject: ServiceId,
    /// SHA-256 of the cert's verifying key (its CRL identity).
    pub fingerprint: [u8; 32],
    /// Start of validity (Unix seconds).
    pub not_before: u64,
    /// End of validity (Unix seconds, exclusive).
    pub not_after: u64,
    /// Whether it has been revoked.
    pub revoked: bool,
}

/// The mesh CA: wraps the IdP and operates the cert lifecycle.
pub struct MeshCa {
    idp: Idp,
    issued: BTreeMap<u64, IssuedCert>,
    revoked_fingerprints: BTreeSet<[u8; 32]>,
    next_serial: u64,
}

impl MeshCa {
    /// Build a mesh CA over an IdP (the CA signing identity + trust anchor).
    pub fn new(idp: Idp) -> Self {
        Self {
            idp,
            issued: BTreeMap::new(),
            revoked_fingerprints: BTreeSet::new(),
            next_serial: 0,
        }
    }

    /// The underlying IdP (for provisioning / token issuance).
    pub fn idp(&self) -> &Idp {
        &self.idp
    }

    /// Mutable IdP access (directory provisioning).
    pub fn idp_mut(&mut self) -> &mut Idp {
        &mut self.idp
    }

    /// Issue a workload certificate valid over `[not_before, not_after)`. Returns the
    /// certificate and its serial. [RC-35]
    pub fn issue(
        &mut self,
        identity: &WorkloadIdentity,
        not_before: u64,
        not_after: u64,
    ) -> Result<(IdentityCert, u64), CaError> {
        let cert = self.idp.certify(identity).map_err(CaError::Sign)?;
        let serial = self.next_serial;
        self.next_serial += 1;
        self.issued.insert(
            serial,
            IssuedCert {
                serial,
                subject: identity.id.clone(),
                fingerprint: cert_fingerprint(&cert),
                not_before,
                not_after,
                revoked: false,
            },
        );
        Ok((cert, serial))
    }

    /// Renew a subject's cert: issue a NEW cert (with a freshly-rekeyed `new_identity`)
    /// over a new window. The old cert is untouched — it keeps working until it
    /// expires or is revoked — so renewal is **downtime-free**. [RC-35]
    pub fn renew(
        &mut self,
        new_identity: &WorkloadIdentity,
        not_before: u64,
        not_after: u64,
    ) -> Result<(IdentityCert, u64), CaError> {
        self.issue(new_identity, not_before, not_after)
    }

    /// Revoke a cert by serial: its fingerprint joins the CRL and every subsequently
    /// built [`MeshCa::trust_anchor`] carries it, so the next handshake rejects it.
    /// [RC-35]
    pub fn revoke(&mut self, serial: u64) -> Result<(), CaError> {
        let rec = self
            .issued
            .get_mut(&serial)
            .ok_or(CaError::UnknownSerial(serial))?;
        rec.revoked = true;
        self.revoked_fingerprints.insert(rec.fingerprint);
        Ok(())
    }

    /// The current trust anchor, carrying the live CRL — hand this to verifiers so
    /// they honour revocations on the next handshake. [RC-35]
    pub fn trust_anchor(&self) -> TrustAnchor {
        self.idp
            .trust_anchor()
            .with_revocations(self.revoked_fingerprints.iter().copied())
    }

    /// The lifecycle status of a cert (by serial) at `now`.
    pub fn status(&self, serial: u64, now: u64) -> CertStatus {
        match self.issued.get(&serial) {
            None => CertStatus::Unknown,
            Some(c) if c.revoked => CertStatus::Revoked,
            Some(c) if now < c.not_before => CertStatus::NotYetValid,
            Some(c) if now >= c.not_after => CertStatus::Expired,
            Some(_) => CertStatus::Valid,
        }
    }

    /// Serials of non-revoked certs that expire at or before `deadline` — the renewal
    /// worklist the operator (or an automated renewer) processes ahead of expiry.
    pub fn expiring_by(&self, deadline: u64) -> Vec<u64> {
        self.issued
            .values()
            .filter(|c| !c.revoked && c.not_after <= deadline)
            .map(|c| c.serial)
            .collect()
    }

    /// The record of an issued cert.
    pub fn issued(&self, serial: u64) -> Option<&IssuedCert> {
        self.issued.get(&serial)
    }
}
