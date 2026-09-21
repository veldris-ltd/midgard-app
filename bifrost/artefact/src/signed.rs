//! Signed artefacts: a manifest + its CycloneDX SBOM + the deployable payload +
//! the detached ML-DSA signature over the manifest. Building computes the payload
//! and SBOM digests and signs; verifying recomputes both digests and checks the
//! signature — so an unsigned or tampered artefact fails closed. [FR-B-1] [FR-B-4]
//! [NFR-PQ-3]
//!
//! ## One acceptance decision
//! The signature check is **not** implemented here. It delegates to
//! [`attest::accept`], the single implementation of "reject unless proven"
//! that the CI release gate, this registry, the offline-bundle import and Mimir's
//! model load all share — so "acceptable" cannot come to mean different things in
//! different places. `[AC-RF-SC-2d]`
//!
//! Delegating also gives Bifrost artefacts **domain separation** they did not
//! have: signatures are made over
//! [`accept::BIFROST_ARTEFACT`](attest::accept::BIFROST_ARTEFACT) ‖
//! `manifest.encode()`, so a Bifrost artefact signature can never be replayed as
//! a bundle manifest or a Mimir model manifest, and vice versa.

use pqc_agility::hash::sha256;

use crate::error::VerifyError;
use crate::manifest::{ArtefactManifest, BIFROST_SIG_ALG};

/// A fully-attested, signed deployable artefact.
#[derive(Debug, Clone)]
pub struct SignedArtefact {
    /// The signed header.
    pub manifest: ArtefactManifest,
    /// The attached CycloneDX SBOM document (digest bound into the manifest).
    pub sbom: String,
    /// The deployable payload bytes (digest bound into the manifest).
    pub payload: Vec<u8>,
    /// The detached ML-DSA signature over `manifest.encode()`.
    pub signature: pqc_agility::Signature,
}

/// Build and sign an artefact: compute the payload + SBOM digests, assemble the
/// manifest, and sign it under `signing_key`. This is the CI publish step (also
/// used by tests). [FR-B-1] [NFR-PQ-3]
#[cfg(feature = "backend-fips")]
pub fn build_and_sign(
    id: impl Into<String>,
    version: impl Into<String>,
    component: impl Into<String>,
    payload: Vec<u8>,
    sbom: String,
    signing_key: &pqc_agility::SigningKey,
) -> SignedArtefact {
    let manifest = ArtefactManifest {
        id: id.into(),
        version: version.into(),
        component: component.into(),
        payload_sha256: sha256(&payload),
        sbom_sha256: sha256(sbom.as_bytes()),
    };
    // Signed through the shared helper so the preimage can never drift from the
    // one `verify` reconstructs.
    let signature = attest::accept::sign_in_domain(
        &attest::accept::BIFROST_ARTEFACT,
        &manifest.encode(),
        signing_key,
    )
    .expect("ml-dsa signing");
    SignedArtefact {
        manifest,
        sbom,
        payload,
        signature,
    }
}

/// Verify a signed artefact against a trusted release verifying key. Fails closed
/// on a tampered payload, a tampered SBOM, or a bad/absent signature — the order
/// checks content integrity first, then the signature. [FR-B-4] [AC-B-1]
#[cfg(feature = "backend-fips")]
pub fn verify(
    trust_key: &pqc_agility::VerifyingKey,
    artefact: &SignedArtefact,
) -> Result<(), VerifyError> {
    // 1. Content integrity: recompute the digests the manifest commits to.
    if sha256(&artefact.payload) != artefact.manifest.payload_sha256 {
        return Err(VerifyError::PayloadDigestMismatch);
    }
    if sha256(artefact.sbom.as_bytes()) != artefact.manifest.sbom_sha256 {
        return Err(VerifyError::SbomDigestMismatch);
    }
    // 2. Authenticity: the one acceptance decision, in the Bifrost artefact
    // domain, restricted to this construct's approved algorithm.
    attest::accept::accept(
        &attest::accept::BIFROST_ARTEFACT,
        &artefact.manifest.encode(),
        trust_key,
        &artefact.signature,
        &[BIFROST_SIG_ALG],
    )
    .map_err(|_| VerifyError::BadSignature)
}
