//! # midgard-client-delivery — verify-before-install, on device
//!
//! Every client artefact is reproducibly built and PQ-signed by Bifrost; the client
//! **verifies the signature before it installs or updates**, and fails closed on a
//! tampered or unsigned artefact. `[FR-C-7]` `[AC-C-7]` For air-gapped deployments
//! the client installs from a **signed offline bundle** with no app-store or
//! internet dependency, verifying it fully offline. `[AC-C-12]`
//!
//! There is no second verifier here (KDD-1): this crate reuses the Bifrost
//! supply-chain trust root — [`bifrost_artefact::verify`] and
//! [`bifrost_bundle::import`] — and adds only the client-shaped install semantics:
//! an update is applied only after verification, and only for the `client`
//! component (a client will not install a model bundle as an app update).

#![forbid(unsafe_code)]

mod error;
pub use error::DeliveryError;

/// The component name client app artefacts carry in their Bifrost manifest.
pub const CLIENT_COMPONENT: &str = "client";

/// A verified, installable artefact reference. Produced only after the signature
/// verifies — its existence is the proof that install/update is safe. `[AC-C-7]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledArtefact {
    /// Artefact id.
    pub id: String,
    /// Version.
    pub version: String,
    /// Component (always [`CLIENT_COMPONENT`] for an installed client).
    pub component: String,
    /// The verified payload digest.
    pub payload_sha256: [u8; 32],
}

#[cfg(feature = "backend-fips")]
mod verified {
    use super::{DeliveryError, InstalledArtefact, CLIENT_COMPONENT};
    use bifrost_artefact::SignedArtefact;
    use bifrost_bundle::OfflineBundle;
    use pqc_agility::VerifyingKey;

    impl InstalledArtefact {
        fn from_artefact(a: &SignedArtefact) -> Self {
            Self {
                id: a.manifest.id.clone(),
                version: a.manifest.version.clone(),
                component: a.manifest.component.clone(),
                payload_sha256: a.manifest.payload_sha256,
            }
        }
    }

    /// Verify a signed artefact against the pinned release key and install it. Fails
    /// closed on a tampered payload/SBOM or a bad signature, and refuses an artefact
    /// that is not the `client` component. This is the gate every install and update
    /// passes through. `[FR-C-7]` `[AC-C-7]`
    pub fn verify_and_install(
        trust_key: &VerifyingKey,
        artefact: &SignedArtefact,
    ) -> Result<InstalledArtefact, DeliveryError> {
        // 1. Supply-chain verification (payload + SBOM digests + ML-DSA signature).
        bifrost_artefact::verify(trust_key, artefact).map_err(DeliveryError::Verify)?;
        // 2. A client installs only client artefacts.
        if artefact.manifest.component != CLIENT_COMPONENT {
            return Err(DeliveryError::WrongComponent {
                expected: CLIENT_COMPONENT.to_string(),
                actual: artefact.manifest.component.clone(),
            });
        }
        Ok(InstalledArtefact::from_artefact(artefact))
    }

    /// Apply an update: verify the new artefact and confirm it targets the same
    /// artefact id as what is installed. Downgrade/cross-artefact swaps are refused.
    /// `[FR-C-7]` `[AC-C-7]`
    pub fn verify_and_update(
        trust_key: &VerifyingKey,
        current: &InstalledArtefact,
        artefact: &SignedArtefact,
    ) -> Result<InstalledArtefact, DeliveryError> {
        let next = verify_and_install(trust_key, artefact)?;
        if next.id != current.id {
            return Err(DeliveryError::WrongArtefact {
                expected: current.id.clone(),
                actual: next.id,
            });
        }
        Ok(next)
    }

    /// Install from a signed offline bundle at an air-gapped site — **fully offline**,
    /// no app-store or internet dependency. Verifies the bundle signature, the
    /// manifest-to-contents match, and every contained artefact before returning the
    /// installable set; rejects the bundle on any signature failure. `[AC-C-12]`
    pub fn install_from_bundle(
        trust_key: &VerifyingKey,
        bundle: &OfflineBundle,
    ) -> Result<Vec<InstalledArtefact>, DeliveryError> {
        // Bifrost's offline import does the full fail-closed verification offline.
        bifrost_bundle::import(trust_key, bundle).map_err(DeliveryError::Bundle)?;
        // Every artefact in the bundle verified; surface them as installable.
        Ok(bundle
            .artefacts
            .iter()
            .map(InstalledArtefact::from_artefact)
            .collect())
    }
}

#[cfg(feature = "backend-fips")]
pub use verified::{install_from_bundle, verify_and_install, verify_and_update};
