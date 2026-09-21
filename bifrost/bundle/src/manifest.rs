//! The bundle manifest: the signed table of contents of an offline release bundle.
//! It lists, for every contained artefact, the identity and the payload/SBOM
//! digests — so the bundle signature commits to *exactly* which artefacts (and
//! which bytes) travel across the air gap. [FR-B-5]

use bifrost_artefact::SignedArtefact;

/// One entry in a bundle: the identity + digests of a contained artefact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleEntry {
    /// Artefact id.
    pub id: String,
    /// Artefact version.
    pub version: String,
    /// Component kind (e.g. `service`, `client`, `model-bundle`).
    pub component: String,
    /// SHA-256 of the artefact payload.
    pub payload_sha256: [u8; 32],
    /// SHA-256 of the artefact SBOM.
    pub sbom_sha256: [u8; 32],
}

impl BundleEntry {
    /// Derive the entry that describes a signed artefact.
    pub fn of(artefact: &SignedArtefact) -> Self {
        Self {
            id: artefact.manifest.id.clone(),
            version: artefact.manifest.version.clone(),
            component: artefact.manifest.component.clone(),
            payload_sha256: artefact.manifest.payload_sha256,
            sbom_sha256: artefact.manifest.sbom_sha256,
        }
    }
}

/// The signed table of contents of an offline bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleManifest {
    /// Stable bundle id.
    pub bundle_id: String,
    /// A caller-supplied provenance label (e.g. a release tag). No clock is read —
    /// the suite forbids ambient time — so the label is passed in and signed as-is.
    pub label: String,
    /// The contained artefacts, in order.
    pub entries: Vec<BundleEntry>,
}

impl BundleManifest {
    /// Canonical byte encoding signed by the release key (hand-rolled, no serde).
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        let put_str = |o: &mut Vec<u8>, s: &str| {
            o.extend_from_slice(&(s.len() as u32).to_le_bytes());
            o.extend_from_slice(s.as_bytes());
        };
        put_str(&mut o, &self.bundle_id);
        put_str(&mut o, &self.label);
        o.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            put_str(&mut o, &e.id);
            put_str(&mut o, &e.version);
            put_str(&mut o, &e.component);
            o.extend_from_slice(&e.payload_sha256);
            o.extend_from_slice(&e.sbom_sha256);
        }
        o
    }
}
