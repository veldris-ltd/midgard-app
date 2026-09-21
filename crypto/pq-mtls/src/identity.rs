//! Workload identities, certificates, and trust anchors. A certificate is a [NFR-SEC-1]
//! workload identity whose canonical encoding is signed by a certificate
//! authority (the trust anchor).

use std::collections::BTreeSet;

use policy::Role;
use pqc_agility::hash::sha256;
use pqc_agility::{CryptoProvider, SigId, Signature, VerifyingKey};

use crate::error::MtlsError;

/// A service/workload name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceId(pub String);

/// A workload's identity: name, roles, and its signature verifying key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadIdentity {
    /// Service id.
    pub id: ServiceId,
    /// Roles this workload holds (used for authorization).
    pub roles: BTreeSet<Role>,
    /// Signature algorithm of the verifying key.
    pub sig_alg: SigId,
    /// The workload's verifying key.
    pub verifying_key: VerifyingKey,
}

/// A certificate: an identity signed by a CA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityCert {
    /// The certified identity.
    pub identity: WorkloadIdentity,
    /// Algorithm of the CA signature.
    pub ca_sig_alg: SigId,
    /// CA signature over the canonical identity.
    pub ca_signature: Signature,
}

/// A trusted certificate authority, plus the revocation list (CRL) the mesh honours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustAnchor {
    /// CA signature algorithm.
    pub ca_alg: SigId,
    /// CA verifying key.
    pub ca_key: VerifyingKey,
    /// Revoked workload **verifying-key fingerprints** (SHA-256). A cert whose key is
    /// listed here fails the *next* handshake — this is the distributed CRL. Because
    /// renewal rekeys (a new cert has a new key), the fingerprint uniquely names one
    /// issued cert, so revoking one does not revoke its renewals. [RC-35]
    pub revoked: BTreeSet<[u8; 32]>,
}

impl TrustAnchor {
    /// A trust anchor with an empty CRL.
    pub fn new(ca_alg: SigId, ca_key: VerifyingKey) -> Self {
        Self {
            ca_alg,
            ca_key,
            revoked: BTreeSet::new(),
        }
    }

    /// Add revoked fingerprints (the CA distributes the current CRL to verifiers).
    pub fn with_revocations(mut self, fps: impl IntoIterator<Item = [u8; 32]>) -> Self {
        self.revoked.extend(fps);
        self
    }

    /// Revoke one cert by its verifying-key fingerprint.
    pub fn revoke(&mut self, fingerprint: [u8; 32]) {
        self.revoked.insert(fingerprint);
    }

    /// Whether a fingerprint is revoked.
    pub fn is_revoked(&self, fingerprint: &[u8; 32]) -> bool {
        self.revoked.contains(fingerprint)
    }
}

/// The revocation fingerprint of a cert: SHA-256 of its workload verifying key.
pub fn cert_fingerprint(cert: &IdentityCert) -> [u8; 32] {
    sha256(cert.identity.verifying_key.as_bytes())
}

/// Canonical, injective encoding of a workload identity (for cert signing and
/// transcript binding).
pub fn canonical_identity(id: &WorkloadIdentity) -> Vec<u8> {
    let mut out = Vec::new();
    let idb = id.id.0.as_bytes();
    out.extend_from_slice(&(idb.len() as u32).to_le_bytes());
    out.extend_from_slice(idb);
    out.extend_from_slice(&(id.roles.len() as u32).to_le_bytes());
    for r in &id.roles {
        let rb = r.0.as_bytes();
        out.extend_from_slice(&(rb.len() as u32).to_le_bytes());
        out.extend_from_slice(rb);
    }
    out.push(id.sig_alg.as_u8());
    let vk = id.verifying_key.as_bytes();
    out.extend_from_slice(&(vk.len() as u32).to_le_bytes());
    out.extend_from_slice(vk);
    out
}

/// Verify a certificate chains to the trust anchor **and is not revoked**. Fails
/// closed: an untrusted chain or a revoked cert is rejected — so revocation is
/// honoured on the next handshake. [RC-35]
pub fn verify_cert(
    cert: &IdentityCert,
    anchor: &TrustAnchor,
    provider: &CryptoProvider,
) -> Result<(), MtlsError> {
    let scheme = provider
        .sig_for(cert.ca_sig_alg)
        .map_err(|_| MtlsError::PeerCertUntrusted)?;
    scheme
        .verify(
            &anchor.ca_key,
            &canonical_identity(&cert.identity),
            &cert.ca_signature,
        )
        .map_err(|_| MtlsError::PeerCertUntrusted)?;
    // Revocation gate: a revoked cert never establishes a session. [RC-35]
    if anchor.is_revoked(&cert_fingerprint(cert)) {
        return Err(MtlsError::PeerCertRevoked);
    }
    Ok(())
}
