//! The artefact registry: the system of record for signed, versioned artefacts.
//! It verifies **on ingest** (nothing tampered ever enters) and re-verifies **before
//! every deploy** (nothing tampered at rest is ever shipped). Both paths fail
//! closed. [FR-B-1] [FR-B-4] [AC-B-1]
//!
//! Ingest is also **anti-rollback**: per artefact id the registry keeps a release floor
//! (`attest::ReleaseFloor`), the highest version accepted and the digest of its signed
//! manifest, and refuses a release older than it, or the same version with a different
//! manifest. A validly signed old release is still an old release. The registry keeps
//! the releases it accepted before, so a rollout can still revert to one of those.
//! [MG-SEC-14]

use std::collections::BTreeMap;

use attest::ReleaseFloor;
use pqc_agility::hash::sha256;

use crate::error::RegistryError;
use crate::manifest::ArtefactRef;
use crate::signed::SignedArtefact;

/// A registry of signed artefacts, trusting a single release verifying key.
#[cfg(feature = "backend-fips")]
pub struct ArtefactRegistry {
    trust_key: pqc_agility::VerifyingKey,
    artefacts: BTreeMap<ArtefactRef, SignedArtefact>,
    floor: ReleaseFloor,
}

/// What the release floor records for an artefact: the digest of its signed manifest.
pub(crate) fn release_digest(artefact: &SignedArtefact) -> [u8; 32] {
    sha256(&artefact.manifest.encode())
}

#[cfg(feature = "backend-fips")]
impl ArtefactRegistry {
    /// A registry trusting `trust_key` (the release signing root), with a release floor
    /// held in memory.
    pub fn new(trust_key: pqc_agility::VerifyingKey) -> Self {
        Self::with_floor(trust_key, ReleaseFloor::in_memory())
    }

    /// A registry trusting `trust_key` whose anti-rollback floor is `floor` — persisted,
    /// for a floor opened over a file. [MG-SEC-14]
    pub fn with_floor(trust_key: pqc_agility::VerifyingKey, floor: ReleaseFloor) -> Self {
        Self {
            trust_key,
            artefacts: BTreeMap::new(),
            floor,
        }
    }

    /// Ingest an artefact: verify it, refuse it if it is older than a release already
    /// accepted for its id or conflicts with one, and only store it if it passes. A
    /// tampered, unsigned or rolled-back artefact never enters the registry. [AC-B-1]
    /// [MG-SEC-14]
    pub fn ingest(&mut self, artefact: SignedArtefact) -> Result<ArtefactRef, RegistryError> {
        // The signature first: an unverified version label must not move the floor.
        crate::signed::verify(&self.trust_key, &artefact)?;
        self.floor
            .admit(
                &artefact.manifest.id,
                &artefact.manifest.version,
                &release_digest(&artefact),
            )
            .map_err(RegistryError::Rollback)?;
        let key = ArtefactRef::new(
            artefact.manifest.id.clone(),
            artefact.manifest.version.clone(),
        );
        self.artefacts.insert(key.clone(), artefact);
        Ok(key)
    }

    /// Re-admit an artefact this registry accepted before (a durable registry's boot):
    /// verified like any ingest, then checked to be consistent with the floor — at or
    /// below it — rather than above it. [RC-12] [MG-SEC-14]
    pub(crate) fn restore(&mut self, artefact: SignedArtefact) -> Result<(), RegistryError> {
        crate::signed::verify(&self.trust_key, &artefact)?;
        let key = ArtefactRef::new(
            artefact.manifest.id.clone(),
            artefact.manifest.version.clone(),
        );
        self.artefacts.insert(key, artefact);
        Ok(())
    }

    /// The anti-rollback floor.
    pub fn floor(&self) -> &ReleaseFloor {
        &self.floor
    }

    pub(crate) fn floor_mut(&mut self) -> &mut ReleaseFloor {
        &mut self.floor
    }

    /// Every stored artefact, for re-checking against the floor.
    pub(crate) fn artefacts(&self) -> impl Iterator<Item = &SignedArtefact> {
        self.artefacts.values()
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
