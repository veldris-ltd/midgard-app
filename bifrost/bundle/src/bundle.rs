//! Producing and consuming signed offline release bundles. A bundle is built and
//! signed on the connected side; an air-gapped site **imports it fully offline** —
//! verifying the bundle signature, that the manifest matches the contents, and
//! every contained artefact — before anything is deployed. Rejected on any
//! signature failure. [FR-B-5] [AC-B-3]
//!
//! This is the transit path for Mimir model bundles (AC-MS-3) and client artefacts
//! (AC-C-7): they travel as `model-bundle` / `client` component artefacts inside
//! this same signed-bundle workflow and are rejected offline if their signature
//! fails.

use bifrost_artefact::{ArtefactRef, SignedArtefact};

use crate::error::BundleError;
use crate::manifest::{BundleEntry, BundleManifest};

/// A signed, offline-transportable release bundle.
#[derive(Debug, Clone)]
pub struct OfflineBundle {
    /// The signed table of contents.
    pub manifest: BundleManifest,
    /// The contained signed artefacts (aligned to `manifest.entries`).
    pub artefacts: Vec<SignedArtefact>,
    /// The ML-DSA signature over `manifest.encode()`.
    pub signature: pqc_agility::Signature,
}

/// Build and sign an offline bundle from a set of already-signed artefacts. The
/// bundle manifest commits to each artefact's identity and digests, and is signed
/// under the release key. [FR-B-5]
#[cfg(feature = "backend-fips")]
pub fn build_bundle(
    bundle_id: impl Into<String>,
    label: impl Into<String>,
    artefacts: Vec<SignedArtefact>,
    signing_key: &pqc_agility::SigningKey,
) -> OfflineBundle {
    let entries = artefacts.iter().map(BundleEntry::of).collect();
    let manifest = BundleManifest {
        bundle_id: bundle_id.into(),
        label: label.into(),
        entries,
    };
    // A bundle manifest is its own signing context: signed under the bundle
    // domain, so a bundle signature can never be replayed as an artefact
    // signature (or the reverse) even though both use the same key. [AC-RF-SC-2d]
    let signature = attest::accept::sign_in_domain(
        &attest::accept::BIFROST_BUNDLE,
        &manifest.encode(),
        signing_key,
    )
    .expect("ml-dsa signing");
    OfflineBundle {
        manifest,
        artefacts,
        signature,
    }
}

/// Import a bundle at an air-gapped site — **fully offline**. Verifies, in order:
/// (1) the bundle signature, (2) that the manifest matches the contained artefacts,
/// and (3) every contained artefact against the trust key. Returns the imported
/// artefact refs, or fails closed. Nothing is deployed unless all three pass.
/// [FR-B-5] [AC-B-3] [AC-MS-3] [AC-C-7]
#[cfg(feature = "backend-fips")]
pub fn import(
    trust_key: &pqc_agility::VerifyingKey,
    bundle: &OfflineBundle,
) -> Result<Vec<ArtefactRef>, BundleError> {
    // 1. Bundle signature — offline, over the manifest, through the one
    // acceptance decision in the bundle domain.
    attest::accept::accept(
        &attest::accept::BIFROST_BUNDLE,
        &bundle.manifest.encode(),
        trust_key,
        &bundle.signature,
        &[bifrost_artefact::BIFROST_SIG_ALG],
    )
    .map_err(|_| BundleError::BadBundleSignature)?;

    // 2. Manifest matches contents (count + per-entry digests).
    if bundle.manifest.entries.len() != bundle.artefacts.len() {
        return Err(BundleError::ManifestMismatch);
    }
    for (entry, artefact) in bundle.manifest.entries.iter().zip(&bundle.artefacts) {
        if *entry != BundleEntry::of(artefact) {
            return Err(BundleError::ManifestMismatch);
        }
    }

    // 3. Every contained artefact verifies (payload + SBOM digests + signature).
    let mut refs = Vec::with_capacity(bundle.artefacts.len());
    for (index, artefact) in bundle.artefacts.iter().enumerate() {
        bifrost_artefact::verify(trust_key, artefact)
            .map_err(|error| BundleError::Artefact { index, error })?;
        refs.push(ArtefactRef::new(
            artefact.manifest.id.clone(),
            artefact.manifest.version.clone(),
        ));
    }
    Ok(refs)
}
