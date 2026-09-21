//! On-device model loading with PQ-signature verification. A Tier-1 model is
//! loaded **only** if its ML-DSA signature over the model digest verifies against
//! the pinned Veldris key; any mismatch fails closed and the model is refused.
//! `[NFR-PQ-3]` `[AC-C-10]`
//!
//! This is the same discipline the model-supply chain applies in the enclosure
//! (AC-MS-2), enforced again at the edge: a tampered or unsigned on-device model
//! never loads.

use pqc_agility::hash::sha256;
use pqc_agility::{CryptoProvider, Signature, VerifyingKey};

use crate::error::InferenceError;

/// A candidate on-device model bundle: its id, raw bytes, and detached signature.
#[derive(Debug, Clone)]
pub struct SignedModel<'a> {
    /// Stable model id (family/variant).
    pub id: String,
    /// The model bytes (weights/manifest) the signature covers.
    pub bytes: &'a [u8],
    /// The detached ML-DSA signature over the model digest.
    pub signature: Signature,
}

/// A model whose signature verified — the only handle the runtime will load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedModel {
    /// The model id.
    pub id: String,
    /// The verified SHA-256 digest of the model bytes.
    pub digest: [u8; 32],
}

/// Verify a model's PQ signature and return a loadable handle. Fails closed with
/// [`InferenceError::ModelSignatureInvalid`] on any mismatch, and with
/// [`InferenceError::SignatureBackendUnavailable`] if the signature algorithm is
/// not enabled. `[AC-C-10]`
pub fn verify_and_load(
    model: &SignedModel<'_>,
    verifying_key: &VerifyingKey,
    provider: &CryptoProvider,
) -> Result<VerifiedModel, InferenceError> {
    let scheme = provider
        .sig_for(verifying_key.alg())
        .map_err(|_| InferenceError::SignatureBackendUnavailable)?;
    let digest = sha256(model.bytes);
    scheme
        .verify(verifying_key, &digest, &model.signature)
        .map_err(|_| InferenceError::ModelSignatureInvalid)?;
    Ok(VerifiedModel {
        id: model.id.clone(),
        digest,
    })
}
