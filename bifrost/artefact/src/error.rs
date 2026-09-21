//! Verification and registry errors. Every deploy-path failure is explicit and
//! fails closed: an artefact that does not verify is never stored and never
//! deployed.

/// Why an artefact failed verification (all fail closed). [FR-B-4]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyError {
    /// The payload bytes do not hash to the digest in the signed manifest —
    /// the deployable was tampered with after signing.
    PayloadDigestMismatch,
    /// The attached SBOM does not hash to the digest in the signed manifest —
    /// the SBOM was tampered with after signing.
    SbomDigestMismatch,
    /// The ML-DSA signature over the manifest did not verify (unsigned, wrong key,
    /// or tampered manifest).
    BadSignature,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m = match self {
            VerifyError::PayloadDigestMismatch => {
                "payload digest does not match the signed manifest"
            }
            VerifyError::SbomDigestMismatch => "SBOM digest does not match the signed manifest",
            VerifyError::BadSignature => "ML-DSA signature did not verify",
        };
        write!(f, "{m}")
    }
}

impl std::error::Error for VerifyError {}

/// Registry errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// The artefact failed verification on ingest or before deploy.
    Verify(VerifyError),
    /// No artefact registered under the requested id/version.
    NotFound {
        /// Requested id.
        id: String,
        /// Requested version.
        version: String,
    },
    /// The durable registry failed to persist or recover an artefact (I/O or a
    /// malformed on-disk record). Fails closed. [RC-12]
    Io(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::Verify(e) => write!(f, "verification failed: {e}"),
            RegistryError::NotFound { id, version } => {
                write!(f, "no artefact {id}@{version} in the registry")
            }
            RegistryError::Io(m) => write!(f, "durable registry i/o: {m}"),
        }
    }
}

impl std::error::Error for RegistryError {}

impl From<VerifyError> for RegistryError {
    fn from(e: VerifyError) -> Self {
        RegistryError::Verify(e)
    }
}
