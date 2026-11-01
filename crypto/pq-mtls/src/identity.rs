//! Workload identities, certificates, and trust anchors. A certificate is a [NFR-SEC-1]
//! workload identity plus its **terms** — serial, validity window and granted
//! clearance — whose canonical encoding is signed by a certificate authority (the
//! trust anchor).
//!
//! Version 2 of the certificate puts the terms inside the signed body: a copied
//! certificate stops working when its window closes [MG-SEC-02], and the clearance a
//! peer is authorised at comes from what the CA granted, never from what the peer
//! asks for [MG-SEC-04].

use std::collections::BTreeSet;
use std::sync::{Arc, RwLock};

use policy::{ClassificationLevel, Role};
use pqc_agility::hash::sha256;
use pqc_agility::{CryptoError, CryptoProvider, SigId, Signature, SigningKey, VerifyingKey};

use crate::error::MtlsError;
use crate::posture::Posture;

/// Domain separation for the signed certificate body (version 2).
const CERT_BODY_TAG: &[u8] = b"midgard-pqmtls-cert-v2";

/// A service/workload name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// What the CA grants alongside an identity, all of it covered by the CA signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertTerms {
    /// The CA's serial for this certificate.
    pub serial: u64,
    /// Start of validity (Unix seconds, inclusive). [MG-SEC-02]
    pub not_before: u64,
    /// End of validity (Unix seconds, exclusive). [MG-SEC-02]
    pub not_after: u64,
    /// The highest clearance the holder may be authorised at. A peer that asks for
    /// more in its handshake is held to this. [MG-SEC-04]
    pub clearance: Posture,
}

impl CertTerms {
    /// Terms valid over `[not_before, not_after)` granting `clearance`.
    pub fn new(serial: u64, not_before: u64, not_after: u64, clearance: Posture) -> Self {
        Self {
            serial,
            not_before,
            not_after,
            clearance,
        }
    }

    /// Whether `now` falls inside the validity window.
    pub fn is_current(&self, now: u64) -> bool {
        self.not_before <= now && now < self.not_after
    }
}

/// A certificate: an identity and its terms, signed by a CA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityCert {
    /// The certified identity.
    pub identity: WorkloadIdentity,
    /// The serial, validity window and granted clearance.
    pub terms: CertTerms,
    /// Algorithm of the CA signature.
    pub ca_sig_alg: SigId,
    /// CA signature over [`canonical_cert_body`].
    pub ca_signature: Signature,
}

impl IdentityCert {
    /// The revocation fingerprint of this cert (see [`cert_fingerprint`]).
    pub fn fingerprint(&self) -> [u8; 32] {
        cert_fingerprint(self)
    }
}

/// Sign `identity` under `terms` with the CA key `ca_key` — how a CA issues a
/// certificate.
pub fn certify(
    provider: &CryptoProvider,
    ca_alg: SigId,
    ca_key: &SigningKey,
    identity: &WorkloadIdentity,
    terms: CertTerms,
) -> Result<IdentityCert, CryptoError> {
    if terms.not_after <= terms.not_before {
        return Err(CryptoError::InvalidKey);
    }
    let ca_signature = provider
        .sig_for(ca_alg)?
        .sign(ca_key, &canonical_cert_body(identity, &terms))?;
    Ok(IdentityCert {
        identity: identity.clone(),
        terms,
        ca_sig_alg: ca_alg,
        ca_signature,
    })
}

/// Revoked fingerprints and the version of the signed list they came from.
#[derive(Debug, Default)]
struct Revocations {
    /// Version of the last signed list installed (0: none yet).
    version: u64,
    revoked: BTreeSet<[u8; 32]>,
}

/// A trusted certificate authority, plus the revocation state the mesh honours.
///
/// The revocation state is **shared by every clone** of an anchor: a service hands
/// clones to each connection, and installing a newer signed list (see
/// [`TrustAnchor::install_crl`] and [`crate::revocation`]) takes effect on the next
/// handshake or resumption on all of them, without a restart. [MG-SEC-03]
#[derive(Clone)]
pub struct TrustAnchor {
    /// CA signature algorithm.
    pub ca_alg: SigId,
    /// CA verifying key.
    pub ca_key: VerifyingKey,
    revocations: Arc<RwLock<Revocations>>,
}

impl TrustAnchor {
    /// A trust anchor with an empty revocation list.
    pub fn new(ca_alg: SigId, ca_key: VerifyingKey) -> Self {
        Self {
            ca_alg,
            ca_key,
            revocations: Arc::new(RwLock::new(Revocations::default())),
        }
    }

    /// Add revoked fingerprints known in-process — the CA's own view, which it then
    /// signs into a list for verifiers. Applies to every clone of this anchor. A
    /// verifier loading revocations from disk uses [`TrustAnchor::install_crl`], which
    /// only accepts a list the CA signed.
    pub fn with_revocations(self, fps: impl IntoIterator<Item = [u8; 32]>) -> Self {
        self.write().revoked.extend(fps);
        self
    }

    /// Revoke one cert by its verifying-key fingerprint (in-process; see
    /// [`TrustAnchor::with_revocations`]).
    pub fn revoke(&self, fingerprint: [u8; 32]) {
        self.write().revoked.insert(fingerprint);
    }

    /// Whether a fingerprint is revoked.
    pub fn is_revoked(&self, fingerprint: &[u8; 32]) -> bool {
        self.read().revoked.contains(fingerprint)
    }

    /// A snapshot of the revoked fingerprints.
    pub fn revoked(&self) -> BTreeSet<[u8; 32]> {
        self.read().revoked.clone()
    }

    /// The version of the signed revocation list currently installed (0 if none).
    pub fn crl_version(&self) -> u64 {
        self.read().version
    }

    /// Install a signed revocation list. It must verify under this anchor's CA key and
    /// must not be older than the list already installed: an attacker who can write
    /// the list file cannot forge one, and cannot roll a revocation back by restoring
    /// an earlier signed list. Returns whether the state changed (an identical
    /// re-install is a no-op). [MG-SEC-03]
    pub fn install_crl(
        &self,
        crl: &crate::revocation::RevocationList,
        provider: &CryptoProvider,
    ) -> Result<bool, MtlsError> {
        crl.verify(self, provider)?;
        let mut state = self.write();
        if crl.version < state.version {
            return Err(MtlsError::CrlRollback {
                installed: state.version,
                offered: crl.version,
            });
        }
        if crl.version == state.version {
            if state.version != 0 && !crl.revoked.is_subset(&state.revoked) {
                return Err(MtlsError::CrlRollback {
                    installed: state.version,
                    offered: crl.version,
                });
            }
            return Ok(false);
        }
        state.version = crl.version;
        // The signed list is authoritative; in-process revocations are kept too, so
        // installing a list never un-revokes anything this process already refuses.
        state.revoked.extend(crl.revoked.iter().copied());
        Ok(true)
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Revocations> {
        self.revocations
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Revocations> {
        self.revocations
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl PartialEq for TrustAnchor {
    fn eq(&self, other: &Self) -> bool {
        self.ca_alg == other.ca_alg
            && self.ca_key == other.ca_key
            && self.read().revoked == other.read().revoked
    }
}
impl Eq for TrustAnchor {}

impl std::fmt::Debug for TrustAnchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let r = self.read();
        f.debug_struct("TrustAnchor")
            .field("ca_alg", &self.ca_alg)
            .field("ca_key", &self.ca_key)
            .field("crl_version", &r.version)
            .field("revoked", &r.revoked.len())
            .finish()
    }
}

/// The revocation fingerprint of a cert: SHA-256 of its workload verifying key.
/// Renewal rekeys, so the fingerprint names exactly one issued cert and revoking it
/// does not revoke its renewals. [RC-35]
pub fn cert_fingerprint(cert: &IdentityCert) -> [u8; 32] {
    sha256(cert.identity.verifying_key.as_bytes())
}

/// Canonical, injective encoding of a workload identity.
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

/// Canonical, injective encoding of what the CA signs: a version tag, the identity
/// and its terms. Also folded into the handshake transcript, so the terms each side
/// saw are bound to the session.
pub fn canonical_cert_body(id: &WorkloadIdentity, terms: &CertTerms) -> Vec<u8> {
    let mut out = CERT_BODY_TAG.to_vec();
    out.extend_from_slice(&canonical_identity(id));
    out.extend_from_slice(&terms.serial.to_le_bytes());
    out.extend_from_slice(&terms.not_before.to_le_bytes());
    out.extend_from_slice(&terms.not_after.to_le_bytes());
    out.push(terms.clearance.level.0);
    out.extend_from_slice(&(terms.clearance.compartments.len() as u32).to_le_bytes());
    for c in &terms.clearance.compartments {
        let cb = c.0.as_bytes();
        out.extend_from_slice(&(cb.len() as u32).to_le_bytes());
        out.extend_from_slice(cb);
    }
    out
}

/// Verify a certificate chains to the trust anchor, **is inside its validity window
/// at `now`**, and **is not revoked**. Fails closed: an untrusted chain, an expired or
/// not-yet-valid cert, or a revoked cert is rejected — on every handshake. [RC-35]
/// [MG-SEC-02] [MG-SEC-03]
pub fn verify_cert(
    cert: &IdentityCert,
    anchor: &TrustAnchor,
    provider: &CryptoProvider,
    now: u64,
) -> Result<(), MtlsError> {
    if cert.ca_sig_alg != anchor.ca_alg {
        return Err(MtlsError::PeerCertUntrusted);
    }
    let scheme = provider
        .sig_for(cert.ca_sig_alg)
        .map_err(|_| MtlsError::PeerCertUntrusted)?;
    scheme
        .verify(
            &anchor.ca_key,
            &canonical_cert_body(&cert.identity, &cert.terms),
            &cert.ca_signature,
        )
        .map_err(|_| MtlsError::PeerCertUntrusted)?;
    if !cert.terms.is_current(now) {
        return Err(MtlsError::PeerCertExpired {
            not_before: cert.terms.not_before,
            not_after: cert.terms.not_after,
            now,
        });
    }
    // Revocation gate: a revoked cert never establishes a session. [RC-35]
    if anchor.is_revoked(&cert_fingerprint(cert)) {
        return Err(MtlsError::PeerCertRevoked);
    }
    Ok(())
}

/// The clearance a peer is authorised at: the lower of what its certificate grants and
/// what it requested — the lower level and only the compartments in both. A peer can
/// ask for less than it was granted, never more. [MG-SEC-04]
pub fn effective_clearance(granted: &Posture, requested: &Posture) -> Posture {
    Posture {
        level: ClassificationLevel(granted.level.0.min(requested.level.0)),
        compartments: granted
            .compartments
            .intersection(&requested.compartments)
            .cloned()
            .collect(),
    }
}
