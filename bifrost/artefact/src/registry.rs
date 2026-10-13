//! The artefact registry: the system of record for signed, versioned artefacts.
//! It verifies **on ingest** (nothing tampered ever enters) and re-verifies **before
//! every deploy** (nothing tampered at rest is ever shipped). Both paths fail
//! closed. [FR-B-1] [FR-B-4] [AC-B-1]

use std::collections::BTreeMap;

use crate::error::RegistryError;
use crate::manifest::ArtefactRef;
use crate::signed::SignedArtefact;

/// A registry of signed artefacts, trusting a single release verifying key.
#[cfg(feature = "backend-fips")]
pub struct ArtefactRegistry {
    trust_key: pqc_agility::VerifyingKey,
    artefacts: BTreeMap<ArtefactRef, SignedArtefact>,
}

#[cfg(feature = "backend-fips")]
impl ArtefactRegistry {
    /// A registry trusting `trust_key` (the release signing root).
    pub fn new(trust_key: pqc_agility::VerifyingKey) -> Self {
        Self {
            trust_key,
            artefacts: BTreeMap::new(),
        }
    }

    /// Ingest an artefact: verify it, and only store it if it passes. A tampered or
    /// unsigned artefact is rejected and never enters the registry. [AC-B-1]
    pub fn ingest(&mut self, artefact: SignedArtefact) -> Result<ArtefactRef, RegistryError> {
        crate::signed::verify(&self.trust_key, &artefact)?;
        let key = ArtefactRef::new(
            artefact.manifest.id.clone(),
            artefact.manifest.version.clone(),
        );
        self.artefacts.insert(key.clone(), artefact);
        Ok(key)
    }

    /// Look up an artefact without re-verifying (for inspection).
    pub fn get(&self, id: &str, version: &str) -> Option<&SignedArtefact> {
        self.artefacts.get(&ArtefactRef::new(id, version))
    }

    /// The deploy gate: fetch an artefact and **re-verify it before handing it to a
    /// rollout**. Even a registry-resident artefact must verify at deploy time, so a
    /// store compromised at rest cannot ship. Fails closed. [FR-B-4] [AC-B-1]
    pub fn verify_for_deploy(
        &self,
        id: &str,
        version: &str,
    ) -> Result<&SignedArtefact, RegistryError> {
        let artefact = self
            .artefacts
            .get(&ArtefactRef::new(id, version))
            .ok_or_else(|| RegistryError::NotFound {
                id: id.to_string(),
                version: version.to_string(),
            })?;
        crate::signed::verify(&self.trust_key, artefact)?;
        Ok(artefact)
    }

    /// Every registered artefact's reference, sorted. This is what makes "what
    /// did the release actually publish?" answerable from the registry itself,
    /// so the published set can be compared with the gated set. [AC-RF-SC-5c]
    pub fn refs(&self) -> Vec<ArtefactRef> {
        self.artefacts.keys().cloned().collect()
    }

    /// Number of registered artefacts.
    pub fn len(&self) -> usize {
        self.artefacts.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.artefacts.is_empty()
    }
}
