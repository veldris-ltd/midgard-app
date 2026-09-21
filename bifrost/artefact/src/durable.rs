//! Durable, integrity-protected artefact registry.
//!
//! Each ingested [`SignedArtefact`] is persisted to its own file under the
//! registry directory (written atomically: temp + `fsync` + rename). Artefacts are
//! public, signed release bundles, so integrity is the artefact's own ML-DSA
//! signature — not encryption: on [`DurableArtefactRegistry::open`] every persisted
//! artefact is decoded and **re-ingested through the same verify-on-ingest path**,
//! so a tampered on-disk artefact fails ML-DSA verification and boot fails closed.
//! [FR-B-1] [FR-B-4] [AC-B-1] [RC-12]

// The module is gated at its declaration in lib.rs; repeating the cfg here is
// the same attribute applied twice (clippy::duplicated_attributes).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use pqc_agility::{SigId, Signature, VerifyingKey};

use crate::error::RegistryError;
use crate::manifest::{ArtefactManifest, ArtefactRef};
use crate::registry::ArtefactRegistry;
use crate::signed::SignedArtefact;

const EXT: &str = "artefact";

/// An [`ArtefactRegistry`] whose contents survive a restart, re-verified on boot.
pub struct DurableArtefactRegistry {
    inner: ArtefactRegistry,
    dir: PathBuf,
}

impl DurableArtefactRegistry {
    /// Open (creating if absent) the registry rooted at `dir`, reloading every
    /// persisted artefact and **re-verifying** it against `trust_key`. A tampered
    /// or unsigned on-disk artefact fails closed here.
    pub fn open(dir: impl AsRef<Path>, trust_key: VerifyingKey) -> Result<Self, RegistryError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir).map_err(io)?;
        let mut inner = ArtefactRegistry::new(trust_key);

        for entry in fs::read_dir(&dir).map_err(io)? {
            let path = entry.map_err(io)?.path();
            if path.extension().and_then(|s| s.to_str()) != Some(EXT) {
                continue;
            }
            let bytes = fs::read(&path).map_err(io)?;
            let artefact = decode_artefact(&bytes)?;
            // Re-verify on boot through the normal ingest path (fails closed).
            inner.ingest(artefact)?;
        }

        Ok(Self { inner, dir })
    }

    /// Ingest (verify + store) an artefact and durably persist it. A rejected
    /// artefact never touches disk. [AC-B-1]
    pub fn ingest(&mut self, artefact: SignedArtefact) -> Result<ArtefactRef, RegistryError> {
        let key = self.inner.ingest(artefact)?;
        let stored = self
            .inner
            .get(&key.id, &key.version)
            .expect("just ingested");
        atomic_write(&self.blob_path(&key), &encode_artefact(stored))?;
        Ok(key)
    }

    /// Look up without re-verifying (inspection).
    pub fn get(&self, id: &str, version: &str) -> Option<&SignedArtefact> {
        self.inner.get(id, version)
    }

    /// The deploy gate: re-verify the artefact before handing it to a rollout.
    pub fn verify_for_deploy(
        &self,
        id: &str,
        version: &str,
    ) -> Result<&SignedArtefact, RegistryError> {
        self.inner.verify_for_deploy(id, version)
    }

    /// Number of registered artefacts.
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn blob_path(&self, key: &ArtefactRef) -> PathBuf {
        // Collision-free filename from the (id, version) pair.
        let mut raw = key.id.clone().into_bytes();
        raw.push(0);
        raw.extend_from_slice(key.version.as_bytes());
        self.dir.join(format!("{}.{EXT}", hex(&raw)))
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RegistryError> {
    let tmp = path.with_extension("tmp");
    {
        let mut opts = OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f: File = opts.open(&tmp).map_err(io)?;
        f.write_all(bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    fs::rename(&tmp, path).map_err(io)?;
    Ok(())
}

fn io(e: std::io::Error) -> RegistryError {
    RegistryError::Io(e.to_string())
}

// --- SignedArtefact codec (no serde; length-prefixed) ---------------------

fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

fn encode_artefact(a: &SignedArtefact) -> Vec<u8> {
    let mut o = Vec::new();
    put_bytes(&mut o, a.manifest.id.as_bytes());
    put_bytes(&mut o, a.manifest.version.as_bytes());
    put_bytes(&mut o, a.manifest.component.as_bytes());
    o.extend_from_slice(&a.manifest.payload_sha256);
    o.extend_from_slice(&a.manifest.sbom_sha256);
    put_bytes(&mut o, a.sbom.as_bytes());
    put_bytes(&mut o, &a.payload);
    o.push(a.signature.alg().as_u8());
    put_bytes(&mut o, a.signature.as_bytes());
    o
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], RegistryError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| RegistryError::Io("codec truncated".into()))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, RegistryError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<usize, RegistryError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn arr32(&mut self) -> Result<[u8; 32], RegistryError> {
        let mut a = [0u8; 32];
        a.copy_from_slice(self.take(32)?);
        Ok(a)
    }
    fn bytes(&mut self) -> Result<Vec<u8>, RegistryError> {
        let n = self.u32()?;
        Ok(self.take(n)?.to_vec())
    }
    fn string(&mut self) -> Result<String, RegistryError> {
        String::from_utf8(self.bytes()?).map_err(|_| RegistryError::Io("codec invalid utf8".into()))
    }
}

fn decode_artefact(b: &[u8]) -> Result<SignedArtefact, RegistryError> {
    let mut r = Reader { b, pos: 0 };
    let manifest = ArtefactManifest {
        id: r.string()?,
        version: r.string()?,
        component: r.string()?,
        payload_sha256: r.arr32()?,
        sbom_sha256: r.arr32()?,
    };
    let sbom = r.string()?;
    let payload = r.bytes()?;
    let alg = SigId::from_u8(r.u8()?).ok_or_else(|| RegistryError::Io("unknown sig alg".into()))?;
    let sig = Signature::new(alg, r.bytes()?);
    if r.pos != b.len() {
        return Err(RegistryError::Io("codec trailing bytes".into()));
    }
    Ok(SignedArtefact {
        manifest,
        sbom,
        payload,
        signature: sig,
    })
}

fn hex(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(LUT[(b >> 4) as usize] as char);
        s.push(LUT[(b & 0x0f) as usize] as char);
    }
    s
}
