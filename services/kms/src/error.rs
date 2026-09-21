//! KMS errors — every variant is fail-closed. [NFR-PQ-2]

/// Errors from sealing/unsealing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KmsError {
    /// The wrap suite (ML-KEM-1024) is not registered in the provider.
    SuiteUnavailable,
    /// A blob was sealed to a different trust root than this KMS holds.
    WrongTrustRoot,
    /// A blob's recorded suite does not match the wrap suite.
    SuiteMismatch,
    /// A KEM (encapsulate/decapsulate) operation failed.
    Kem(String),
    /// AES-256-GCM seal/open failed (tamper or wrong key).
    Aead,
    /// A serialized sealed blob was truncated or malformed.
    Malformed(String),
    /// The hardware token (HSM/TPM) is unavailable or refused an operation.
    HardwareUnavailable(String),
    /// A root attestation failed to verify (bad signature, exportable key, or it
    /// does not bind the presented root). [RC-28]
    AttestationInvalid,
    /// No known trust root matches a blob's `root_key_id` (e.g. a retired root was
    /// dropped from the keyring). [RC-28]
    UnknownRoot(String),
    /// A signing-custody operation failed (algorithm unavailable, inconsistent key
    /// material, or the custodian refused). Fail-closed — no "signed, probably".
    /// Required by the shared signing root (`signing.rs`). [NFR-PQ-3]
    Sign(String),
}

impl std::fmt::Display for KmsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KmsError::SuiteUnavailable => write!(f, "wrap suite (ML-KEM-1024) unavailable"),
            KmsError::WrongTrustRoot => write!(f, "blob sealed to a different trust root"),
            KmsError::SuiteMismatch => write!(f, "blob suite mismatch"),
            KmsError::Kem(s) => write!(f, "kem error: {s}"),
            KmsError::Aead => write!(f, "AES-256-GCM operation failed"),
            KmsError::Malformed(s) => write!(f, "malformed sealed blob: {s}"),
            KmsError::HardwareUnavailable(s) => write!(f, "hardware token unavailable: {s}"),
            KmsError::AttestationInvalid => write!(f, "root attestation failed to verify"),
            KmsError::UnknownRoot(id) => write!(f, "no known trust root with id '{id}'"),
            KmsError::Sign(s) => write!(f, "signing custody error: {s}"),
        }
    }
}

impl std::error::Error for KmsError {}
