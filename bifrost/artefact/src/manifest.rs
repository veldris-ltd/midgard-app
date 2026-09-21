//! The artefact manifest: the small, canonical, signed header that binds an
//! artefact's identity to the **digest of its payload** and the **digest of its
//! SBOM**. Because both digests live inside the signed bytes, tampering with either
//! the deployable or its SBOM invalidates the signature — there is no way to swap
//! content under a valid signature. [FR-B-1] [NFR-PQ-3]

/// The signing algorithm for Bifrost artefacts: ML-DSA-65 (FIPS 204), the suite
/// standard for artefact signing. [NFR-PQ-3]
pub const BIFROST_SIG_ALG: pqc_agility::SigId = pqc_agility::SigId::MlDsa65;

/// The signed header describing one deployable artefact build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtefactManifest {
    /// Stable artefact id, e.g. `forge-pipeline`.
    pub id: String,
    /// Version label, e.g. `1.4.0`.
    pub version: String,
    /// Component kind, e.g. `service`, `client`, `model-bundle`.
    pub component: String,
    /// SHA-256 of the deployable payload bytes.
    pub payload_sha256: [u8; 32],
    /// SHA-256 of the attached CycloneDX SBOM document.
    pub sbom_sha256: [u8; 32],
}

impl ArtefactManifest {
    /// Canonical byte encoding signed by the release key (hand-rolled, no serde) —
    /// matches the suite's `ModelManifest::encode` discipline so signing is uniform.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        let put_str = |o: &mut Vec<u8>, s: &str| {
            o.extend_from_slice(&(s.len() as u32).to_le_bytes());
            o.extend_from_slice(s.as_bytes());
        };
        put_str(&mut o, &self.id);
        put_str(&mut o, &self.version);
        put_str(&mut o, &self.component);
        o.extend_from_slice(&self.payload_sha256);
        o.extend_from_slice(&self.sbom_sha256);
        o
    }
}

/// A stable reference to a registered artefact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArtefactRef {
    /// Artefact id.
    pub id: String,
    /// Version.
    pub version: String,
}

impl ArtefactRef {
    /// Build a reference.
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
        }
    }
}
