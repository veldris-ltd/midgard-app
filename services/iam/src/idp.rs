//! The Identity & Access provider: a self-contained, offline IdP that (1) is the
//! mesh certificate authority — it certifies workload identities so PQ-mTLS peers
//! can authenticate each other — and (2) issues PQ-signed access tokens for
//! authenticated principals. One ML-DSA issuer key roots both. [NFR-SEC-2] [NFR-PRV-1]

use std::collections::BTreeSet;

use policy::Purpose;
use pq_mtls::{certify, CertTerms, IdentityCert, RevocationList, TrustAnchor, WorkloadIdentity};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{CryptoError, CryptoProvider, Rng, SigId, SigningKey, VerifyingKey};

use crate::store::IdentityStore;
use crate::token::{sign_token, TokenClaims, TokenError};

/// The IdP's signature algorithm (real ML-DSA-65 / FIPS 204).
pub const IDP_SIG_ALG: SigId = SigId::MlDsa65;

/// Errors from token issuance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueError {
    /// No such registered subject.
    UnknownSubject,
    /// The subject is not authorised for the requested purpose.
    PurposeNotAuthorised,
    /// Token construction/signing failed.
    Token(TokenError),
}

impl std::fmt::Display for IssueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueError::UnknownSubject => write!(f, "unknown subject"),
            IssueError::PurposeNotAuthorised => write!(f, "purpose not authorised for subject"),
            IssueError::Token(e) => write!(f, "token error: {e}"),
        }
    }
}

impl std::error::Error for IssueError {}

/// A running IdP instance.
pub struct Idp {
    issuer_id: String,
    provider: CryptoProvider,
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    store: IdentityStore,
}

impl Idp {
    /// Generate a fresh IdP with a new ML-DSA issuer key. The RNG is injected so
    /// deployments can seed from `OsRng` and tests can be deterministic. Fails
    /// closed with a [`CryptoError`] if the signature algorithm is unavailable or
    /// keygen fails, rather than panicking at construction. [RT-50]
    pub fn generate(issuer_id: &str, rng: &mut dyn Rng) -> Result<Self, CryptoError> {
        let provider = fips_provider();
        let (verifying_key, signing_key) = provider.sig_for(IDP_SIG_ALG)?.keypair(rng)?;
        Ok(Self {
            issuer_id: issuer_id.to_string(),
            provider,
            signing_key,
            verifying_key,
            store: IdentityStore::new(),
        })
    }

    /// Rebuild an IdP from a persisted issuer keypair — the restart path, so a token
    /// or mesh certificate issued before a restart still verifies after it.
    ///
    /// Fails closed if either key is not the IdP's algorithm, or if the two keys do
    /// not belong together: a probe signature made with `signing_key` must verify under
    /// `verifying_key`, so a mismatched pair is refused here rather than surfacing
    /// later as every token being rejected. [RC-29] [NFR-SEC-2]
    pub fn from_key(
        issuer_id: &str,
        verifying_key: VerifyingKey,
        signing_key: SigningKey,
    ) -> Result<Self, CryptoError> {
        for alg in [verifying_key.alg(), signing_key.alg()] {
            if alg != IDP_SIG_ALG {
                return Err(CryptoError::AlgorithmMismatch {
                    expected: IDP_SIG_ALG.as_str(),
                    got: alg.as_str(),
                });
            }
        }
        let provider = fips_provider();
        let scheme = provider.sig_for(IDP_SIG_ALG)?;
        let probe = b"midgard-idp/issuer-key-probe/v1";
        let signature = scheme.sign(&signing_key, probe)?;
        scheme
            .verify(&verifying_key, probe, &signature)
            .map_err(|_| CryptoError::InvalidKey)?;
        Ok(Self {
            issuer_id: issuer_id.to_string(),
            provider,
            signing_key,
            verifying_key,
            store: IdentityStore::new(),
        })
    }

    /// The IdP's issuer id.
    pub fn issuer_id(&self) -> &str {
        &self.issuer_id
    }

    /// The crypto provider (shared with consumers verifying tokens/certs).
    pub fn provider(&self) -> &CryptoProvider {
        &self.provider
    }

    /// The issuer verifying key — distribute this to token/cert verifiers (the
    /// offline analogue of a JWKS endpoint).
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// The trust anchor for mesh cert verification.
    pub fn trust_anchor(&self) -> TrustAnchor {
        TrustAnchor::new(IDP_SIG_ALG, self.verifying_key.clone())
    }

    /// The identity directory (mutable, for provisioning).
    pub fn store_mut(&mut self) -> &mut IdentityStore {
        &mut self.store
    }

    /// The identity directory (read-only).
    pub fn store(&self) -> &IdentityStore {
        &self.store
    }

    /// Certify a workload identity under `terms`: the IdP signs the identity together
    /// with its serial, validity window and granted clearance, producing a certificate
    /// that chains to [`Idp::trust_anchor`], stops working when the window closes, and
    /// caps the clearance its holder can be authorised at. [MG-SEC-02] [MG-SEC-04]
    pub fn certify(
        &self,
        identity: &WorkloadIdentity,
        terms: CertTerms,
    ) -> Result<IdentityCert, TokenError> {
        certify(
            &self.provider,
            IDP_SIG_ALG,
            &self.signing_key,
            identity,
            terms,
        )
        .map_err(|_| TokenError::BadSignature)
    }

    /// Sign a revocation list: the only form in which verifiers accept revocations.
    /// `version` must be higher than any list issued before. [MG-SEC-03]
    pub fn sign_crl(
        &self,
        version: u64,
        issued_at: u64,
        revoked: BTreeSet<[u8; 32]>,
    ) -> Result<RevocationList, TokenError> {
        RevocationList::sign(
            &self.provider,
            IDP_SIG_ALG,
            &self.signing_key,
            version,
            issued_at,
            revoked,
        )
        .map_err(|_| TokenError::BadSignature)
    }

    /// Issue a token for a registered subject and a declared purpose, valid for
    /// `[now, now + ttl_secs)`. The subject must exist and be authorised for the
    /// purpose (deny-by-default).
    pub fn issue_token(
        &self,
        subject: &str,
        purpose: Purpose,
        now: u64,
        ttl_secs: u64,
    ) -> Result<Vec<u8>, IssueError> {
        let user = self.store.get(subject).ok_or(IssueError::UnknownSubject)?;
        self.mint_token(subject, user, purpose, now, ttl_secs)
    }

    /// Mint a PQ-signed token from an **explicitly supplied** user record — the seam
    /// used by the authenticator and the federation bridge, which resolve the
    /// principal's attributes from the durable directory or an upstream IdP rather
    /// than the in-memory store. The subject must be authorised for the purpose
    /// (deny-by-default). Keeps the existing ML-DSA token format. [RC-29]
    pub fn mint_token(
        &self,
        subject: &str,
        user: &crate::store::UserRecord,
        purpose: Purpose,
        now: u64,
        ttl_secs: u64,
    ) -> Result<Vec<u8>, IssueError> {
        if !user.purposes.contains(&purpose) {
            return Err(IssueError::PurposeNotAuthorised);
        }
        let claims = TokenClaims {
            issuer: self.issuer_id.clone(),
            subject: subject.to_string(),
            roles: user.roles.clone(),
            level: user.clearance.level,
            compartments: user.clearance.compartments.clone(),
            purpose,
            issued_at: now,
            expires_at: now.saturating_add(ttl_secs),
        };
        sign_token(&self.provider, IDP_SIG_ALG, &self.signing_key, &claims)
            .map_err(IssueError::Token)
    }
}
