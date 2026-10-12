//! Streaming bundles: artefacts too large to hold in memory — a model's weights — packed
//! and imported a chunk at a time. [FR-B-5] [AC-B-3] [AC-MS-3]
//!
//! [`crate::build_bundle`] and [`crate::read_bundle`] hold every payload in memory,
//! which suits signed binaries and not a model of several gigabytes.
//! [`pack_streaming`] writes a format version 2 bundle a chunk at a time, and
//! [`import_streaming`] verifies one and writes its artefacts out the same way, holding
//! at most one chunk.
//!
//! Version 2 layout (little-endian, length-prefixed, no serde):
//! ```text
//! "MGBUNDLE" u8:2
//! sig_alg:u8  sig:bytes                          -- the bundle ML-DSA signature
//! bundle_id:str  label:str                       -- signed manifest header
//! entries: u32 count, each { id:str version:str component:str payload[32] sbom[32] }
//! artefacts: u32 count, each {
//!     id:str version:str component:str payload_sha256[32] sbom_sha256[32]
//!     sbom:str
//!     payload_len:u64  chunk_size:u32
//!     ceil(payload_len / chunk_size) chunks, each { chunk_sha256[32] chunk_bytes }
//!     sig_alg:u8  sig:bytes }
//! ```
//!
//! Authenticity is exactly version 1's. The bundle signature covers the manifest, and
//! with it every artefact's payload and SBOM digest; each artefact's own signature
//! covers its manifest. The payload's whole SHA-256, which those signatures bind, is
//! computed as the chunks stream past. A chunk's own SHA-256 is **not** signed: it lets
//! a reader stop at the first damaged chunk of a many-gigabyte payload and say which,
//! rather than read to the end to learn only that something differs.
//!
//! An import writes into a staging directory beside `out_dir` and moves the artefacts
//! into place only after the last one has verified, so a refused import leaves nothing
//! behind, and no import overwrites a file.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use bifrost_artefact::{ArtefactManifest, ArtefactRef, VerifyError, BIFROST_SIG_ALG};
use pqc_agility::hash::{sha256, Digest, Sha256};
use pqc_agility::{SigId, Signature, SigningKey, VerifyingKey};

use crate::bundle::import;
use crate::error::BundleError;
use crate::file::{read_bundle, MAX_CHUNK_SIZE, STREAMING_FORMAT_VERSION};
use crate::manifest::{BundleEntry, BundleManifest};

const MAGIC: &[u8; 8] = b"MGBUNDLE";

/// The chunk size to pack with unless there is a reason for another: 16 MiB.
pub const DEFAULT_CHUNK_SIZE: u32 = 16 * 1024 * 1024;

/// The longest string field (an id, a label, an SBOM) a reader accepts.
const MAX_FIELD: usize = 64 * 1024 * 1024;

/// An artefact to pack, from a file on disk.
#[derive(Debug, Clone)]
pub struct FileArtefact {
    /// Artefact id; on import, the file name it is written to.
    pub id: String,
    /// Version.
    pub version: String,
    /// Component kind (e.g. `service`, `model-bundle`).
    pub component: String,
    /// Its CycloneDX SBOM document.
    pub sbom: String,
    /// The payload file.
    pub path: PathBuf,
}

fn io(e: std::io::Error) -> BundleError {
    BundleError::Io(e.to_string())
}

/// Pack `artefacts` into a signed version 2 bundle at `path`, reading each payload a
/// chunk at a time, twice: once to digest and sign it, once to write it. A payload that
/// changes in between is refused. Writes atomically (temp + rename) and returns the
/// signed manifest. [FR-B-5] [AC-MS-3]
pub fn pack_streaming(
    path: impl AsRef<Path>,
    bundle_id: impl Into<String>,
    label: impl Into<String>,
    artefacts: &[FileArtefact],
    signing_key: &SigningKey,
    chunk_size: u32,
) -> Result<BundleManifest, BundleError> {
    if chunk_size == 0 || chunk_size > MAX_CHUNK_SIZE {
        return Err(BundleError::Io(format!(
            "the chunk size must be 1 to {MAX_CHUNK_SIZE} bytes"
        )));
    }
    // 1. Digest every payload, sign each artefact's manifest, then the bundle's.
    let mut signed = Vec::with_capacity(artefacts.len());
    for artefact in artefacts {
        let (len, payload_sha256) = digest_file(&artefact.path)?;
        let manifest = ArtefactManifest {
            id: artefact.id.clone(),
            version: artefact.version.clone(),
            component: artefact.component.clone(),
            payload_sha256,
            sbom_sha256: sha256(artefact.sbom.as_bytes()),
        };
        let signature = sign(
            &attest::accept::BIFROST_ARTEFACT,
            &manifest.encode(),
            signing_key,
        )?;
        signed.push(Signed {
            manifest,
            len,
            signature,
        });
    }
    let manifest = BundleManifest {
        bundle_id: bundle_id.into(),
        label: label.into(),
        entries: signed.iter().map(|s| entry_of(&s.manifest)).collect(),
    };
    let signature = sign(
        &attest::accept::BIFROST_BUNDLE,
        &manifest.encode(),
        signing_key,
    )?;

    // 2. Write the container, re-reading each payload a chunk at a time.
    let path = path.as_ref();
    let tmp = path.with_extension("tmp");
    match write_container(&tmp, &manifest, &signature, &signed, artefacts, chunk_size) {
        Ok(()) => {
            std::fs::rename(&tmp, path).map_err(io)?;
            Ok(manifest)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Verify the bundle at `path` fully offline and, when `out_dir` is given, write out its
/// artefacts: each payload to `out_dir/<id>` and its SBOM to
/// `out_dir/<id>.sbom.cdx.json`. A version 2 bundle is read a chunk at a time; a
/// version 1 bundle is read whole, as [`crate::read_bundle`] does.
///
/// The checks run in order and each fails closed: the bundle signature (before any
/// payload is read), each record against the signed manifest, each SBOM digest, each
/// chunk digest, each payload digest, each artefact signature, and no trailing bytes.
/// Returns the imported refs; on any refusal nothing is written. [FR-B-5] [AC-B-3]
/// [AC-MS-3]
pub fn import_streaming(
    path: impl AsRef<Path>,
    trust_key: &VerifyingKey,
    out_dir: Option<&Path>,
) -> Result<Vec<ArtefactRef>, BundleError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(io)?;
    let len = file.metadata().map_err(io)?.len();
    let mut r = Source {
        inner: BufReader::with_capacity(1 << 20, file),
        left: len,
    };
    if r.array::<8>()? != *MAGIC {
        return Err(BundleError::Io("bad magic (not an MGBUNDLE file)".into()));
    }
    match r.u8()? {
        1 => return import_whole(path, trust_key, out_dir),
        STREAMING_FORMAT_VERSION => {}
        _ => return Err(BundleError::Io("unsupported bundle format version".into())),
    }

    // The bundle signature, before any payload is read.
    let signature = r.signature()?;
    let bundle_id = r.string()?;
    let label = r.string()?;
    let count = r.u32()?;
    let mut entries = Vec::new();
    for _ in 0..count {
        entries.push(BundleEntry {
            id: r.string()?,
            version: r.string()?,
            component: r.string()?,
            payload_sha256: r.array()?,
            sbom_sha256: r.array()?,
        });
    }
    let manifest = BundleManifest {
        bundle_id,
        label,
        entries,
    };
    attest::accept::accept(
        &attest::accept::BIFROST_BUNDLE,
        &manifest.encode(),
        trust_key,
        &signature,
        &[BIFROST_SIG_ALG],
    )
    .map_err(|_| BundleError::BadBundleSignature)?;

    let staging = match out_dir {
        Some(dir) => Some(Staging::begin(dir, &manifest.entries)?),
        None => None,
    };
    if r.u32()? != manifest.entries.len() {
        return Err(BundleError::ManifestMismatch);
    }
    let mut refs = Vec::with_capacity(manifest.entries.len());
    for (index, entry) in manifest.entries.iter().enumerate() {
        let artefact = ArtefactManifest {
            id: r.string()?,
            version: r.string()?,
            component: r.string()?,
            payload_sha256: r.array()?,
            sbom_sha256: r.array()?,
        };
        if entry_of(&artefact) != *entry {
            return Err(BundleError::ManifestMismatch);
        }
        let sbom = r.string()?;
        if sha256(sbom.as_bytes()) != artefact.sbom_sha256 {
            return Err(BundleError::Artefact {
                index,
                error: VerifyError::SbomDigestMismatch,
            });
        }

        let payload_len = u64::from_le_bytes(r.array()?);
        let chunk_size = u64::from(u32::from_le_bytes(r.array()?));
        if chunk_size == 0 || chunk_size > u64::from(MAX_CHUNK_SIZE) {
            return Err(BundleError::Io("bad chunk size".into()));
        }
        if payload_len > r.left {
            return Err(BundleError::Io("truncated bundle".into()));
        }
        let mut sink = match &staging {
            Some(s) => Some(BufWriter::new(
                File::create(s.dir.join(&artefact.id)).map_err(io)?,
            )),
            None => None,
        };
        let mut chunk = vec![0u8; chunk_size.min(payload_len) as usize];
        let mut whole = Sha256::new();
        let mut left = payload_len;
        let mut number = 0u64;
        while left > 0 {
            let n = left.min(chunk_size) as usize;
            let expected: [u8; 32] = r.array()?;
            r.read_exact(&mut chunk[..n])?;
            if sha256(&chunk[..n]) != expected {
                return Err(BundleError::ChunkDigestMismatch {
                    index,
                    chunk: number,
                });
            }
            whole.update(&chunk[..n]);
            if let Some(sink) = sink.as_mut() {
                sink.write_all(&chunk[..n]).map_err(io)?;
            }
            left -= n as u64;
            number += 1;
        }
        if whole.finalize() != artefact.payload_sha256 {
            return Err(BundleError::Artefact {
                index,
                error: VerifyError::PayloadDigestMismatch,
            });
        }
        let artefact_signature = r.signature()?;
        attest::accept::accept(
            &attest::accept::BIFROST_ARTEFACT,
            &artefact.encode(),
            trust_key,
            &artefact_signature,
            &[BIFROST_SIG_ALG],
        )
        .map_err(|_| BundleError::Artefact {
            index,
            error: VerifyError::BadSignature,
        })?;
        // The same structural SBOM gate `bifrost_artefact::verify` applies, now
        // that the SBOM is known to be the signed one.
        bifrost_artefact::check_sbom(&sbom)
            .map_err(|error| BundleError::Artefact { index, error })?;

        if let Some(mut sink) = sink {
            sink.flush().map_err(io)?;
        }
        if let Some(s) = &staging {
            std::fs::write(s.dir.join(format!("{}.sbom.cdx.json", artefact.id)), &sbom)
                .map_err(io)?;
        }
        refs.push(ArtefactRef::new(artefact.id, artefact.version));
    }
    if r.left != 0 {
        return Err(BundleError::Io("trailing bytes after bundle".into()));
    }
    if let Some(staging) = staging {
        staging.commit()?;
    }
    Ok(refs)
}

/// A version 1 bundle: read whole, verified by [`import`], written out through the same
/// staging as a streamed one.
fn import_whole(
    path: &Path,
    trust_key: &VerifyingKey,
    out_dir: Option<&Path>,
) -> Result<Vec<ArtefactRef>, BundleError> {
    let bundle = read_bundle(path)?;
    let refs = import(trust_key, &bundle)?;
    if let Some(dir) = out_dir {
        let staging = Staging::begin(dir, &bundle.manifest.entries)?;
        for artefact in &bundle.artefacts {
            let id = &artefact.manifest.id;
            std::fs::write(staging.dir.join(id), &artefact.payload)
                .and_then(|()| {
                    std::fs::write(
                        staging.dir.join(format!("{id}.sbom.cdx.json")),
                        &artefact.sbom,
                    )
                })
                .map_err(io)?;
        }
        staging.commit()?;
    }
    Ok(refs)
}

/// One artefact's signed manifest and payload length, between the passes.
struct Signed {
    manifest: ArtefactManifest,
    len: u64,
    signature: Signature,
}

fn entry_of(m: &ArtefactManifest) -> BundleEntry {
    BundleEntry {
        id: m.id.clone(),
        version: m.version.clone(),
        component: m.component.clone(),
        payload_sha256: m.payload_sha256,
        sbom_sha256: m.sbom_sha256,
    }
}

fn sign(
    domain: &attest::accept::Domain,
    body: &[u8],
    signing_key: &SigningKey,
) -> Result<Signature, BundleError> {
    attest::accept::sign_in_domain(domain, body, signing_key)
        .map_err(|e| BundleError::Io(format!("signing failed: {e}")))
}

/// Stream a file through SHA-256: its length and digest.
fn digest_file(path: &Path) -> Result<(u64, [u8; 32]), BundleError> {
    let mut file = File::open(path)
        .map_err(|e| BundleError::Io(format!("cannot open {}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut len = 0u64;
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io(e)),
        };
        hasher.update(&buf[..n]);
        len += n as u64;
    }
    Ok((len, hasher.finalize()))
}

fn write_container(
    tmp: &Path,
    manifest: &BundleManifest,
    signature: &Signature,
    signed: &[Signed],
    artefacts: &[FileArtefact],
    chunk_size: u32,
) -> Result<(), BundleError> {
    let mut w = BufWriter::new(File::create(tmp).map_err(io)?);
    w.write_all(MAGIC).map_err(io)?;
    w.write_all(&[STREAMING_FORMAT_VERSION]).map_err(io)?;
    put_signature(&mut w, signature)?;
    put_bytes(&mut w, manifest.bundle_id.as_bytes())?;
    put_bytes(&mut w, manifest.label.as_bytes())?;
    w.write_all(&(manifest.entries.len() as u32).to_le_bytes())
        .map_err(io)?;
    for e in &manifest.entries {
        put_fields(&mut w, &e.id, &e.version, &e.component)?;
        w.write_all(&e.payload_sha256)
            .and_then(|()| w.write_all(&e.sbom_sha256))
            .map_err(io)?;
    }
    w.write_all(&(signed.len() as u32).to_le_bytes())
        .map_err(io)?;
    let mut chunk = vec![0u8; chunk_size as usize];
    for (s, source) in signed.iter().zip(artefacts) {
        let m = &s.manifest;
        put_fields(&mut w, &m.id, &m.version, &m.component)?;
        w.write_all(&m.payload_sha256)
            .and_then(|()| w.write_all(&m.sbom_sha256))
            .map_err(io)?;
        put_bytes(&mut w, source.sbom.as_bytes())?;
        w.write_all(&s.len.to_le_bytes())
            .and_then(|()| w.write_all(&chunk_size.to_le_bytes()))
            .map_err(io)?;

        let changed = || {
            BundleError::Io(format!(
                "{} changed while it was being packed",
                source.path.display()
            ))
        };
        let mut file = File::open(&source.path).map_err(io)?;
        let mut whole = Sha256::new();
        let mut left = s.len;
        while left > 0 {
            let n = left.min(u64::from(chunk_size)) as usize;
            file.read_exact(&mut chunk[..n]).map_err(|_| changed())?;
            whole.update(&chunk[..n]);
            w.write_all(&sha256(&chunk[..n]))
                .and_then(|()| w.write_all(&chunk[..n]))
                .map_err(io)?;
            left -= n as u64;
        }
        // The file must still be exactly the bytes that were signed.
        let mut extra = [0u8; 1];
        if whole.finalize() != m.payload_sha256 || file.read(&mut extra).map_err(io)? != 0 {
            return Err(changed());
        }
        put_signature(&mut w, &s.signature)?;
    }
    w.flush().map_err(io)?;
    w.into_inner()
        .map_err(|e| io(e.into_error()))?
        .sync_all()
        .map_err(io)
}

fn put_bytes(w: &mut impl Write, bytes: &[u8]) -> Result<(), BundleError> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| BundleError::Io("a field is larger than 4 GiB".into()))?;
    w.write_all(&len.to_le_bytes())
        .and_then(|()| w.write_all(bytes))
        .map_err(io)
}

fn put_fields(
    w: &mut impl Write,
    id: &str,
    version: &str,
    component: &str,
) -> Result<(), BundleError> {
    put_bytes(w, id.as_bytes())?;
    put_bytes(w, version.as_bytes())?;
    put_bytes(w, component.as_bytes())
}

fn put_signature(w: &mut impl Write, signature: &Signature) -> Result<(), BundleError> {
    w.write_all(&[signature.alg().as_u8()]).map_err(io)?;
    put_bytes(w, signature.as_bytes())
}

/// An artefact id names the file it is written to: a plain file name.
fn check_id(id: &str) -> Result<(), BundleError> {
    if id.is_empty() || id.starts_with('.') || id.contains(['/', '\\', ':']) {
        return Err(BundleError::Io(format!(
            "artefact id {id:?} is not a plain file name"
        )));
    }
    Ok(())
}

/// Where an import writes before anything is moved into place: a directory beside the
/// destination, removed unless the import commits.
struct Staging {
    dir: PathBuf,
    out: PathBuf,
    committed: bool,
}

impl Staging {
    fn begin(out: &Path, entries: &[BundleEntry]) -> Result<Self, BundleError> {
        let mut seen = BTreeSet::new();
        for e in entries {
            check_id(&e.id)?;
            if !seen.insert(e.id.as_str()) {
                return Err(BundleError::Io(format!(
                    "the bundle names artefact {} twice",
                    e.id
                )));
            }
            for name in [e.id.clone(), format!("{}.sbom.cdx.json", e.id)] {
                let target = out.join(name);
                if target.exists() {
                    return Err(BundleError::Io(format!(
                        "{} exists; import never overwrites",
                        target.display()
                    )));
                }
            }
        }
        let name = out
            .file_name()
            .ok_or_else(|| BundleError::Io(format!("{} names no directory", out.display())))?;
        let dir = out.with_file_name(format!(".{}.partial", name.to_string_lossy()));
        if dir.exists() {
            // Left by an import that was interrupted.
            std::fs::remove_dir_all(&dir).map_err(io)?;
        }
        std::fs::create_dir_all(&dir).map_err(io)?;
        Ok(Self {
            dir,
            out: out.to_path_buf(),
            committed: false,
        })
    }

    fn commit(mut self) -> Result<(), BundleError> {
        if self.out.exists() {
            for entry in std::fs::read_dir(&self.dir).map_err(io)? {
                let entry = entry.map_err(io)?;
                let target = self.out.join(entry.file_name());
                if target.exists() {
                    return Err(BundleError::Io(format!(
                        "{} exists; import never overwrites",
                        target.display()
                    )));
                }
                std::fs::rename(entry.path(), target).map_err(io)?;
            }
            std::fs::remove_dir(&self.dir).map_err(io)?;
        } else {
            std::fs::rename(&self.dir, &self.out).map_err(io)?;
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// A bundle file being read, with the bytes it has left, so a length field can never
/// make the reader allocate or wait for more than the file holds.
struct Source {
    inner: BufReader<File>,
    left: u64,
}

impl Source {
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), BundleError> {
        if buf.len() as u64 > self.left {
            return Err(BundleError::Io("truncated bundle".into()));
        }
        self.inner
            .read_exact(buf)
            .map_err(|_| BundleError::Io("truncated bundle".into()))?;
        self.left -= buf.len() as u64;
        Ok(())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], BundleError> {
        let mut a = [0u8; N];
        self.read_exact(&mut a)?;
        Ok(a)
    }

    fn u8(&mut self) -> Result<u8, BundleError> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Result<usize, BundleError> {
        Ok(u32::from_le_bytes(self.array()?) as usize)
    }

    fn bytes(&mut self) -> Result<Vec<u8>, BundleError> {
        let n = self.u32()?;
        if n > MAX_FIELD || n as u64 > self.left {
            return Err(BundleError::Io("an oversized or truncated field".into()));
        }
        let mut v = vec![0u8; n];
        self.read_exact(&mut v)?;
        Ok(v)
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
