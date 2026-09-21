//! Minting and checking the panel's per-subsystem service identities.

use std::collections::BTreeSet;

use iam::{Idp, TokenError};
use pq_mtls::{verify_cert, IdentityCert, ServiceId, TrustAnchor, WorkloadIdentity};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::{CryptoError, CryptoProvider, Rng, SigId, SignatureScheme};

use crate::subsystem::{service_role, SUBSYSTEMS};

/// The panel's identities are ML-DSA-65 workload certs (matches the IdP's issuer alg).
const ALG: SigId = SigId::MlDsa65;

/// A failure while minting the per-subsystem identities — surfaced instead of
/// panicking at construction, so a keygen or certification failure is recoverable.
/// [RT-50]
#[derive(Debug)]
pub enum IdentityError {
    /// A subsystem keypair could not be generated.
    Keygen(CryptoError),
    /// The IdP could not certify a workload identity.
    Certify(TokenError),
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdentityError::Keygen(e) => write!(f, "subsystem keygen failed: {e}"),
            IdentityError::Certify(e) => write!(f, "IdP certification failed: {e}"),
        }
    }
}

impl std::error::Error for IdentityError {}

/// One subsystem binding: the subsystem name and the panel's certified identity for it.
#[derive(Debug, Clone)]
pub struct SubsystemIdentity {
    /// The subsystem this identity administers.
    pub subsystem: String,
    /// The certified workload identity the panel presents to that subsystem.
    pub cert: IdentityCert,
}

/// The panel's full set of per-subsystem service identities.
pub struct AdminServiceIdentities {
    anchor: TrustAnchor,
    identities: Vec<SubsystemIdentity>,
}

impl AdminServiceIdentities {
    /// Mint one distinct, least-privilege workload identity per managed subsystem,
    /// each certified by the IdP. Each identity carries exactly one narrow service
    /// role; **no superuser identity is minted**. `[AC-AD-9]`
    pub fn issue(idp: &Idp, rng: &mut dyn Rng) -> Result<Self, IdentityError> {
        let mut identities = Vec::new();
        for sub in SUBSYSTEMS {
            // A fresh, distinct keypair per subsystem — bindings share no key material.
            let (verifying_key, _signing_key) = FipsSig::new(ALG)
                .keypair(rng)
                .map_err(IdentityError::Keygen)?;
            let mut roles = BTreeSet::new();
            roles.insert(service_role(sub)); // exactly ONE narrow service role
            let identity = WorkloadIdentity {
                id: ServiceId(format!("admin-panel->{sub}")),
                roles,
                sig_alg: ALG,
                verifying_key,
            };
            let cert = idp.certify(&identity).map_err(IdentityError::Certify)?;
            identities.push(SubsystemIdentity {
                subsystem: (*sub).to_string(),
                cert,
            });
        }
        Ok(Self {
            anchor: idp.trust_anchor(),
            identities,
        })
    }

    /// The certified identity for a subsystem, if the panel holds one.
    pub fn cert_for(&self, subsystem: &str) -> Option<&IdentityCert> {
        self.identities
            .iter()
            .find(|i| i.subsystem == subsystem)
            .map(|i| &i.cert)
    }

    /// All per-subsystem identities.
    pub fn identities(&self) -> &[SubsystemIdentity] {
        &self.identities
    }

    /// The trust anchor these identities chain to.
    pub fn trust_anchor(&self) -> &TrustAnchor {
        &self.anchor
    }

    /// Whether `cert` is authorised to administer `subsystem`: it must chain to the
    /// trust anchor **and** carry that subsystem's service role. A cert minted for
    /// one subsystem is therefore rejected by every other. `[AC-AD-9]`
    pub fn authorized_for(
        &self,
        cert: &IdentityCert,
        subsystem: &str,
        provider: &CryptoProvider,
    ) -> bool {
        verify_cert(cert, &self.anchor, provider).is_ok()
            && cert.identity.roles.contains(&service_role(subsystem))
    }

    /// Whether the identity set is least-privilege: every identity carries exactly
    /// one role, and none carries a root / superuser / wildcard role. `[AC-AD-9]`
    pub fn is_least_privilege(&self) -> bool {
        self.identities.iter().all(|i| {
            i.cert.identity.roles.len() == 1
                && i.cert
                    .identity
                    .roles
                    .iter()
                    .all(|r| !is_superuser_role(&r.0))
        })
    }
}

/// Whether a role name denotes an all-powerful / cross-cutting privilege.
fn is_superuser_role(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("root") || n.contains("superuser") || n.contains("admin-all") || n == "*"
}
