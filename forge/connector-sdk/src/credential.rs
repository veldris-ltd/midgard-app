//! Credentials by reference, never by value. A connector holds only a
//! [`CredentialRef`]; the secret bytes are resolved at sync time from the KMS
//! (unsealed from an at-rest blob) and never persisted in the connector or the
//! dataset. This is the SAD §11 `Source → Connector` control: the connector runs
//! sandboxed and the credential comes from the KMS. [FR-F-2] [NFR-PQ-2]

use kms::{Kms, SealedBlob, TrustRoot};
use std::collections::BTreeMap;

use crate::error::ConnectorError;

/// A stable, non-secret handle naming a credential (e.g. `pg://sales-ro`). What it
/// refers to is resolved out-of-band; the ref itself is safe to log.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CredentialRef(pub String);

impl CredentialRef {
    /// Construct a credential reference.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

/// Resolved secret material. Its `Debug` is redacted so a secret never lands in a
/// log line or error message by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Vec<u8>);

impl Secret {
    /// Wrap resolved bytes.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    /// Borrow the secret bytes (use at the point of authentication only).
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret(<{} bytes redacted>)", self.0.len())
    }
}

/// Resolves a [`CredentialRef`] to [`Secret`] material at sync time.
pub trait CredentialResolver {
    /// Resolve a credential, or fail if it is unknown / cannot be unsealed.
    fn resolve(&self, cref: &CredentialRef) -> Result<Secret, ConnectorError>;
}

/// A resolver for connectors that need no credential (offline files, in-memory
/// fixtures). Any resolve attempt fails, so declaring a credential you don't wire
/// up is caught rather than silently ignored.
pub struct NoCredentials;

impl CredentialResolver for NoCredentials {
    fn resolve(&self, cref: &CredentialRef) -> Result<Secret, ConnectorError> {
        Err(ConnectorError::Credential(format!(
            "no credentials configured (requested '{}')",
            cref.0
        )))
    }
}

/// A KMS-backed resolver: credentials are held **sealed at rest** and unsealed on
/// demand through the trust root. The plaintext exists only transiently inside
/// [`resolve`]. [NFR-PQ-2]
pub struct KmsCredentialResolver<'a, R: TrustRoot> {
    kms: &'a Kms<R>,
    sealed: BTreeMap<CredentialRef, SealedBlob>,
}

impl<'a, R: TrustRoot> KmsCredentialResolver<'a, R> {
    /// A resolver over a KMS with no credentials yet.
    pub fn new(kms: &'a Kms<R>) -> Self {
        Self {
            kms,
            sealed: BTreeMap::new(),
        }
    }

    /// Register a sealed credential under a reference. The AAD binds the seal to the
    /// reference name, so a blob cannot be swapped between refs.
    pub fn insert_sealed(&mut self, cref: CredentialRef, sealed: SealedBlob) {
        self.sealed.insert(cref, sealed);
    }
}

impl<'a, R: TrustRoot> CredentialResolver for KmsCredentialResolver<'a, R> {
    fn resolve(&self, cref: &CredentialRef) -> Result<Secret, ConnectorError> {
        let blob = self.sealed.get(cref).ok_or_else(|| {
            ConnectorError::Credential(format!("unknown credential '{}'", cref.0))
        })?;
        let plaintext = self
            .kms
            .unseal(cref.0.as_bytes(), blob)
            .map_err(|e| ConnectorError::Credential(format!("unseal failed: {e:?}")))?;
        Ok(Secret::new(plaintext))
    }
}
