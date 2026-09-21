//! The dataset & versioning store: content-addressed, immutable snapshots sealed
//! at rest, with named branch refs and full lineage. Every committed snapshot is
//! AES-256-GCM-encrypted under an ML-KEM-1024-wrapped key via the KMS — a raw
//! ingest is sealed exactly like a curated output. [FR-F-3] [NFR-PQ-2] [AC-F-1]
//!
//! The store owns no key material: it holds a [`Kms`] over a caller-provided trust
//! root, so a hardware root (HSM/TPM) drops in unchanged. Content ids are derived
//! from *plaintext* canonical bytes, so reproducibility (AC-F-2) is independent of
//! the fresh per-seal nonce — two independent stores commit the same data to the
//! same content id. [AC-F-2]

use std::collections::BTreeMap;

use kms::{Kms, SealedBlob, TrustRoot};
use pqc_agility::hash::sha256;
use pqc_agility::Rng;

use crate::codec;
use crate::dataset::Dataset;
use crate::error::StoreError;
use crate::lineage::Lineage;
use crate::version::{ContentId, DatasetVersion};

/// A stored snapshot: its version header plus the sealed bytes at rest.
#[derive(Debug, Clone)]
pub struct StoredVersion {
    /// The addressable, lineage-bearing header.
    pub version: DatasetVersion,
    /// The dataset's canonical bytes, sealed (carries no plaintext). [NFR-PQ-2]
    pub sealed: SealedBlob,
}

/// A content-addressed, PQ-sealed dataset store with branch refs.
pub struct DatasetStore<R: TrustRoot> {
    kms: Kms<R>,
    versions: BTreeMap<ContentId, StoredVersion>,
    branches: BTreeMap<String, ContentId>,
}

impl<R: TrustRoot> DatasetStore<R> {
    /// Build a store over a KMS bound to a trust root.
    pub fn new(kms: Kms<R>) -> Self {
        Self {
            kms,
            versions: BTreeMap::new(),
            branches: BTreeMap::new(),
        }
    }

    /// Commit a dataset as a new immutable version and move `branch` to it. The
    /// snapshot is sealed at rest; the content id is bound into the seal as AAD so
    /// a blob cannot be swapped between versions. Committing identical data is
    /// idempotent in identity — the content id is unchanged — but re-seals with a
    /// fresh nonce. [FR-F-3] [NFR-PQ-2] [AC-F-1]
    pub fn commit(
        &mut self,
        dataset: &Dataset,
        branch: &str,
        lineage: Lineage,
        rng: &mut dyn Rng,
    ) -> Result<DatasetVersion, StoreError> {
        let canonical = codec::encode(dataset);
        let content_id = ContentId(sha256(&canonical));

        let mut schema_bytes = Vec::new();
        dataset.schema().encode(&mut schema_bytes);
        let schema_hash = sha256(&schema_bytes);

        let version = DatasetVersion {
            content_id,
            schema_hash,
            parents: lineage.inputs.clone(),
            lineage,
            row_count: dataset.row_count(),
        };

        // Seal the canonical bytes, binding the content id as AAD.
        let sealed = self.kms.seal(content_id.as_bytes(), &canonical, rng)?;

        self.versions.insert(
            content_id,
            StoredVersion {
                version: version.clone(),
                sealed,
            },
        );
        self.branches.insert(branch.to_string(), content_id);
        Ok(version)
    }

    /// Rehydrate a previously-sealed version into the store WITHOUT re-sealing —
    /// the durable backing store uses this to reload committed blobs on boot. The
    /// version keeps its original content id and sealed bytes. [RC-11]
    pub(crate) fn insert_stored(&mut self, stored: StoredVersion) {
        self.versions.insert(stored.version.content_id, stored);
    }

    /// Point a branch at a content id (used by the durable store when reloading
    /// refs on boot). [RC-11]
    pub(crate) fn set_branch(&mut self, branch: &str, id: ContentId) {
        self.branches.insert(branch.to_string(), id);
    }

    /// Recover a committed dataset by content id: unseal (fails closed on the wrong
    /// root or a tampered blob) and decode.
    pub fn checkout(&self, content_id: &ContentId) -> Result<Dataset, StoreError> {
        let stored = self
            .versions
            .get(content_id)
            .ok_or(StoreError::UnknownVersion)?;
        let plaintext = self.kms.unseal(content_id.as_bytes(), &stored.sealed)?;
        codec::decode(&plaintext)
    }

    /// Recover the dataset at a branch's head.
    pub fn checkout_branch(&self, branch: &str) -> Result<Dataset, StoreError> {
        let id = self.branch_head(branch).ok_or(StoreError::UnknownVersion)?;
        self.checkout(&id)
    }

    /// The content id a branch currently points at.
    pub fn branch_head(&self, branch: &str) -> Option<ContentId> {
        self.branches.get(branch).copied()
    }

    /// The stored header + sealed blob for a version.
    pub fn get(&self, content_id: &ContentId) -> Option<&StoredVersion> {
        self.versions.get(content_id)
    }

    /// Whether a version is present.
    pub fn contains(&self, content_id: &ContentId) -> bool {
        self.versions.contains_key(content_id)
    }

    /// Number of distinct committed versions.
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    /// Whether the store holds no versions.
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }
}
