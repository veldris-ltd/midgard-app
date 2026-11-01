//! The bundle manifest: the signed table of contents of an offline release bundle.
//! It lists, for every contained artefact, the identity and the payload/SBOM
//! digests — so the bundle signature commits to *exactly* which artefacts (and
//! which bytes) travel across the air gap. [FR-B-5]

use bifrost_artefact::SignedArtefact;

use crate::error::BundleError;

/// The largest bundle manifest a reader takes in before its signature has been checked.
/// [MG-SEC-14]
pub const MAX_MANIFEST: usize = 16 * 1024 * 1024;

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

impl BundleManifest {
    /// Decode a manifest from exactly the bytes [`encode`](Self::encode) writes. Call it
    /// only on bytes whose bundle signature has verified: the signature is checked over
    /// these raw bytes, and only then are they decoded. Strict: every byte belongs to a
    /// field, and the result re-encodes to the same bytes. [MG-SEC-14]
    pub fn decode(raw: &[u8]) -> Result<Self, BundleError> {
        let mut r = Frame { b: raw, p: 0 };
        let bundle_id = r.string()?;
        let label = r.string()?;
        let count = r.u32()?;
        let mut entries = Vec::new();
        for _ in 0..count {
            entries.push(BundleEntry {
                id: r.string()?,
                version: r.string()?,
                component: r.string()?,
                payload_sha256: r.digest()?,
                sbom_sha256: r.digest()?,
            });
        }
        let manifest = Self {
            bundle_id,
            label,
            entries,
        };
        if manifest.encode() != raw {
            return Err(BundleError::Io("malformed bundle manifest".into()));
        }
        Ok(manifest)
    }
}

/// How many bytes the manifest at the start of `b` occupies, following only its length
/// prefixes — the framing a reader needs to find the bytes a bundle signature covers
/// without decoding any of them. At most [`MAX_MANIFEST`] bytes. [MG-SEC-14]
pub(crate) fn manifest_extent(b: &[u8]) -> Result<usize, BundleError> {
    let mut r = Frame {
        b: &b[..b.len().min(MAX_MANIFEST)],
        p: 0,
    };
    r.field()?; // bundle id
    r.field()?; // label
    for _ in 0..r.u32()? {
        for _ in 0..3 {
            r.field()?; // id, version, component
        }
        r.take(64)?; // payload and SBOM digests
    }
    Ok(r.p)
}

/// A position in a byte slice, every step bounds-checked.
struct Frame<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Frame<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BundleError> {
        let end = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| BundleError::Io("truncated or oversized bundle manifest".into()))?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }

    fn u32(&mut self) -> Result<usize, BundleError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }

    fn field(&mut self) -> Result<&'a [u8], BundleError> {
        let n = self.u32()?;
        self.take(n)
    }

    fn string(&mut self) -> Result<String, BundleError> {
        String::from_utf8(self.field()?.to_vec())
            .map_err(|_| BundleError::Io("malformed bundle manifest".into()))
    }

    fn digest(&mut self) -> Result<[u8; 32], BundleError> {
        Ok(<[u8; 32]>::try_from(self.take(32)?).expect("32 bytes"))
    }
}
