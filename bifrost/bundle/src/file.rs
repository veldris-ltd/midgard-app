//! The on-disk bundle container format — a single, self-describing, signed file an
//! [`OfflineBundle`] serializes to and is read back from **fully offline**.
//!
//! Layout (little-endian, length-prefixed, no serde):
//! ```text
//! "MGBUNDLE" u8:version
//! sig_alg:u8  sig:bytes                          -- the bundle ML-DSA signature
//! bundle_id:str  label:str                       -- signed manifest header
//! entries: u32 count, each { id:str version:str component:str payload[32] sbom[32] }
//! artefacts: u32 count, each SignedArtefact {
//!     id:str version:str component:str payload_sha256[32] sbom_sha256[32]
//!     sbom:str  payload:bytes  sig_alg:u8  sig:bytes }
//! ```
//! [`read_bundle`] reconstructs the exact [`BundleManifest`] that was signed, so a
//! tampered file fails the signature/manifest/artefact checks in [`crate::import`].
//! Reading itself fails closed on a bad magic or truncation. [FR-B-5] [AC-B-3] [RC-19]
//!
//! [`read_bundle`] and [`from_bytes`] also read format version 2, the chunked layout
//! `crate::stream` writes, checking each chunk's digest as they reassemble the payload
//! in memory. Large bundles are better imported with `crate::import_streaming`.

use std::path::Path;

use bifrost_artefact::{ArtefactManifest, SignedArtefact};
use pqc_agility::hash::sha256;
use pqc_agility::{SigId, Signature};

use crate::bundle::OfflineBundle;
use crate::error::BundleError;
use crate::manifest::{BundleEntry, BundleManifest};

const MAGIC: &[u8; 8] = b"MGBUNDLE";
const FORMAT_VERSION: u8 = 1;

/// The format version of a bundle whose payloads are written in chunks.
pub const STREAMING_FORMAT_VERSION: u8 = 2;

/// The largest chunk a version 2 bundle may declare, bounding what one chunk can make a
/// reader allocate.
pub const MAX_CHUNK_SIZE: u32 = 256 * 1024 * 1024;

/// Serialize a bundle to its on-disk container bytes.
pub fn to_bytes(bundle: &OfflineBundle) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend_from_slice(MAGIC);
    o.push(FORMAT_VERSION);
    // Bundle signature (over the manifest).
    o.push(bundle.signature.alg().as_u8());
    put_bytes(&mut o, bundle.signature.as_bytes());
    // Signed manifest header.
    put_str(&mut o, &bundle.manifest.bundle_id);
    put_str(&mut o, &bundle.manifest.label);
    o.extend_from_slice(&(bundle.manifest.entries.len() as u32).to_le_bytes());
    for e in &bundle.manifest.entries {
        put_str(&mut o, &e.id);
        put_str(&mut o, &e.version);
        put_str(&mut o, &e.component);
        o.extend_from_slice(&e.payload_sha256);
        o.extend_from_slice(&e.sbom_sha256);
    }
    // Contained artefacts.
    o.extend_from_slice(&(bundle.artefacts.len() as u32).to_le_bytes());
    for a in &bundle.artefacts {
        put_str(&mut o, &a.manifest.id);
        put_str(&mut o, &a.manifest.version);
        put_str(&mut o, &a.manifest.component);
        o.extend_from_slice(&a.manifest.payload_sha256);
        o.extend_from_slice(&a.manifest.sbom_sha256);
        put_str(&mut o, &a.sbom);
        put_bytes(&mut o, &a.payload);
        o.push(a.signature.alg().as_u8());
        put_bytes(&mut o, a.signature.as_bytes());
    }
    o
}

/// Parse on-disk container bytes back into a bundle. Fails closed on a bad magic,
/// unknown version, truncation, or trailing bytes.
pub fn from_bytes(b: &[u8]) -> Result<OfflineBundle, BundleError> {
    let mut r = Reader { b, p: 0 };
    if r.take(8)? != MAGIC {
        return Err(BundleError::Io("bad magic (not an MGBUNDLE file)".into()));
    }
    let version = r.u8()?;
    if version != FORMAT_VERSION && version != STREAMING_FORMAT_VERSION {
        return Err(BundleError::Io("unsupported bundle format version".into()));
    }
    let signature = r.signature()?;
    let bundle_id = r.string()?;
    let label = r.string()?;
    let nentries = r.u32()?;
    let mut entries = Vec::with_capacity(nentries);
    for _ in 0..nentries {
        entries.push(BundleEntry {
            id: r.string()?,
            version: r.string()?,
            component: r.string()?,
            payload_sha256: r.arr32()?,
            sbom_sha256: r.arr32()?,
        });
    }
    let nart = r.u32()?;
    let mut artefacts = Vec::with_capacity(nart);
    for index in 0..nart {
        let manifest = ArtefactManifest {
            id: r.string()?,
            version: r.string()?,
            component: r.string()?,
            payload_sha256: r.arr32()?,
            sbom_sha256: r.arr32()?,
        };
        let sbom = r.string()?;
        let payload = if version == FORMAT_VERSION {
            r.bytes()?
        } else {
            r.chunked(index)?
        };
        let signature = r.signature()?;
        artefacts.push(SignedArtefact {
            manifest,
            sbom,
            payload,
            signature,
        });
    }
    if r.p != b.len() {
        return Err(BundleError::Io("trailing bytes after bundle".into()));
    }
    Ok(OfflineBundle {
        manifest: BundleManifest {
            bundle_id,
            label,
            entries,
        },
        artefacts,
        signature,
    })
}

/// Write a bundle to `path` (atomically: temp + rename).
pub fn write_bundle(path: impl AsRef<Path>, bundle: &OfflineBundle) -> Result<(), BundleError> {
    let path = path.as_ref();
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, to_bytes(bundle)).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)?;
    Ok(())
}

/// Read a bundle from `path` — pure file I/O, no network.
pub fn read_bundle(path: impl AsRef<Path>) -> Result<OfflineBundle, BundleError> {
    from_bytes(&std::fs::read(path).map_err(io)?)
}

fn io(e: std::io::Error) -> BundleError {
    BundleError::Io(e.to_string())
}

fn put_str(o: &mut Vec<u8>, s: &str) {
    put_bytes(o, s.as_bytes());
}
fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BundleError> {
        let e = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| BundleError::Io("truncated bundle".into()))?;
        let s = &self.b[self.p..e];
        self.p = e;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, BundleError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<usize, BundleError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn arr32(&mut self) -> Result<[u8; 32], BundleError> {
        let mut a = [0u8; 32];
        a.copy_from_slice(self.take(32)?);
        Ok(a)
    }
    fn bytes(&mut self) -> Result<Vec<u8>, BundleError> {
        let n = self.u32()?;
        Ok(self.take(n)?.to_vec())
    }
    /// A payload written in chunks (format version 2), each chunk checked against the
    /// digest recorded beside it as it is reassembled.
    fn chunked(&mut self, index: usize) -> Result<Vec<u8>, BundleError> {
        let len = u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes"));
        let size = self.u32()?;
        if size == 0 || size > MAX_CHUNK_SIZE as usize {
            return Err(BundleError::Io("bad chunk size".into()));
        }
        let len = usize::try_from(len)
            .ok()
            .filter(|&len| len <= self.b.len() - self.p)
            .ok_or_else(|| BundleError::Io("truncated bundle".into()))?;
        let mut payload = Vec::with_capacity(len);
        let mut chunk = 0u64;
        while payload.len() < len {
            let digest = self.arr32()?;
            let n = (len - payload.len()).min(size);
            let bytes = self.take(n)?;
            if sha256(bytes) != digest {
                return Err(BundleError::ChunkDigestMismatch { index, chunk });
            }
            payload.extend_from_slice(bytes);
            chunk += 1;
        }
        Ok(payload)
    }
    fn string(&mut self) -> Result<String, BundleError> {
        String::from_utf8(self.bytes()?).map_err(|_| BundleError::Io("invalid utf8".into()))
    }
    fn signature(&mut self) -> Result<Signature, BundleError> {
        let alg = SigId::from_u8(self.u8()?)
            .ok_or_else(|| BundleError::Io("unknown signature algorithm".into()))?;
        Ok(Signature::new(alg, self.bytes()?))
    }
}
