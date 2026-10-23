//! The durable, content-addressed backing store for [`DatasetStore`].
//!
//! Layout under the store directory:
//! - `objects/<hex-content-id>` — one immutable, KMS-sealed version blob per
//!   committed snapshot (the `StoredVersion`: its lineage header + the sealed
//!   dataset bytes). Content-addressed and write-once, written atomically
//!   (temp + rename) so a crash mid-commit never leaves a partial blob.
//! - `refs/<hex-branch-name>` — one file per branch holding the head content-id
//!   hex, updated **atomically** (temp + `fsync` + rename).
//!
//! On [`DurableDatasetStore::open`] every blob is loaded (the filename is checked
//! against the header's content id — a misplaced/renamed blob fails closed) and
//! every ref is rehydrated (a dangling ref fails closed) into an in-memory
//! [`DatasetStore`], so a restart recovers every version, branch, and lineage. The
//! crypto and content-addressing are unchanged — this only persists what the store
//! already produced. [SAD_01 §13] [RC-11]

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use kms::{Kms, SealedBlob, TrustRoot};
use pqc_agility::Rng;

use crate::dataset::Dataset;
use crate::error::StoreError;
use crate::lineage::Lineage;
use crate::store::{DatasetStore, StoredVersion};
use crate::version::{ContentId, DatasetVersion};

/// A [`DatasetStore`] whose versions and branch refs persist to disk.
pub struct DurableDatasetStore<R: TrustRoot> {
    inner: DatasetStore<R>,
    objects_dir: PathBuf,
    refs_dir: PathBuf,
}

impl<R: TrustRoot> DurableDatasetStore<R> {
    /// Open (creating if absent) the durable store rooted at `dir`, reloading every
    /// committed version and branch ref into memory. Fails closed on a corrupt
    /// blob, a blob whose filename disagrees with its content id, or a dangling
    /// branch ref.
    pub fn open(dir: impl AsRef<Path>, kms: Kms<R>) -> Result<Self, StoreError> {
        let dir = dir.as_ref();
        let objects_dir = dir.join("objects");
        let refs_dir = dir.join("refs");
        fs::create_dir_all(&objects_dir).map_err(io)?;
        fs::create_dir_all(&refs_dir).map_err(io)?;

        let mut inner = DatasetStore::new(kms);

        // 1. Load every immutable version blob.
        for entry in fs::read_dir(&objects_dir).map_err(io)? {
            let path = entry.map_err(io)?.path();
            if !path.is_file() {
                continue;
            }
            let bytes = fs::read(&path).map_err(io)?;
            let stored = decode_stored(&bytes)?;
            // Integrity: the filename IS the content id (content-addressed).
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name != stored.version.content_id.to_hex() {
                return Err(StoreError::Io(format!(
                    "blob {name} does not match its content id"
                )));
            }
            inner.insert_stored(stored);
        }

        // 2. Rehydrate branch refs; a ref to an absent version fails closed.
        for entry in fs::read_dir(&refs_dir).map_err(io)? {
            let path = entry.map_err(io)?.path();
            if !path.is_file() {
                continue;
            }
            let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let branch_bytes = hex_decode(fname)?;
            let branch = String::from_utf8(branch_bytes)
                .map_err(|_| StoreError::Io("branch name not utf8".into()))?;
            let cid_hex = fs::read_to_string(&path).map_err(io)?;
            let cid = content_id_from_hex(cid_hex.trim())?;
            if !inner.contains(&cid) {
                return Err(StoreError::Io(format!(
                    "branch {branch:?} points at an absent version"
                )));
            }
            inner.set_branch(&branch, cid);
        }

        Ok(Self {
            inner,
            objects_dir,
            refs_dir,
        })
    }

    /// Commit a dataset (sealing at rest, unchanged), then durably persist the new
    /// version blob and atomically move `branch` to it. [FR-F-3] [RC-11]
    pub fn commit(
        &mut self,
        dataset: &Dataset,
        branch: &str,
        lineage: Lineage,
        rng: &mut dyn Rng,
    ) -> Result<DatasetVersion, StoreError> {
        let version = self.inner.commit(dataset, branch, lineage, rng)?;
        let stored = self
            .inner
            .get(&version.content_id)
            .expect("just committed")
            .clone();

        // Immutable, content-addressed blob: write-once, atomically.
        let blob_path = self.objects_dir.join(version.content_id.to_hex());
        if !blob_path.exists() {
            atomic_write(&blob_path, &encode_stored(&stored))?;
        }
        // Atomic ref update (temp + fsync + rename).
        let ref_path = self.refs_dir.join(hex(branch.as_bytes()));
        atomic_write(&ref_path, version.content_id.to_hex().as_bytes())?;
        Ok(version)
    }

    /// Recover the dataset at a branch's head (unseals — fails closed on tamper).
    pub fn checkout_branch(&self, branch: &str) -> Result<Dataset, StoreError> {
        self.inner.checkout_branch(branch)
    }
    /// Recover a committed dataset by content id.
    pub fn checkout(&self, content_id: &ContentId) -> Result<Dataset, StoreError> {
        self.inner.checkout(content_id)
    }
    /// The content id a branch points at.
    pub fn branch_head(&self, branch: &str) -> Option<ContentId> {
        self.inner.branch_head(branch)
    }
    /// The stored header + sealed blob for a version.
    pub fn get(&self, content_id: &ContentId) -> Option<&StoredVersion> {
        self.inner.get(content_id)
    }
    /// Number of distinct committed versions.
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    /// Whether the store holds no versions.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// Write `bytes` to `path` atomically: a sibling temp file, `fsync`, then rename.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let tmp = path.with_extension("tmp");
    {
        let mut opts = OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        // On Windows the file inherits its folder's ACL: the app's working copy, %LOCALAPPDATA%\Midgard, is private to the user (the W2 shell checks it).
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f: File = opts.open(&tmp).map_err(io)?;
        f.write_all(bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    fs::rename(&tmp, path).map_err(io)?; // atomic replace on POSIX
    Ok(())
}

fn io(e: std::io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

// --- StoredVersion codec (no serde; SealedBlob has its own encode/decode) ---

fn encode_stored(s: &StoredVersion) -> Vec<u8> {
    let mut vb = Vec::new();
    encode_version(&mut vb, &s.version);
    let mut o = Vec::with_capacity(4 + vb.len());
    o.extend_from_slice(&(vb.len() as u32).to_le_bytes());
    o.extend_from_slice(&vb);
    o.extend_from_slice(&s.sealed.encode()); // consumes the remainder on decode
    o
}

fn decode_stored(b: &[u8]) -> Result<StoredVersion, StoreError> {
    if b.len() < 4 {
        return Err(StoreError::Io("truncated stored version".into()));
    }
    let vlen = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let vend = 4usize
        .checked_add(vlen)
        .filter(|&e| e <= b.len())
        .ok_or_else(|| StoreError::Io("bad version length".into()))?;
    let version = decode_version(&b[4..vend])?;
    let sealed = SealedBlob::decode(&b[vend..])
        .map_err(|e| StoreError::Io(format!("sealed blob: {e:?}")))?;
    Ok(StoredVersion { version, sealed })
}

fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

fn encode_version(o: &mut Vec<u8>, v: &DatasetVersion) {
    o.extend_from_slice(&v.content_id.0);
    o.extend_from_slice(&v.schema_hash);
    o.extend_from_slice(&(v.parents.len() as u32).to_le_bytes());
    for p in &v.parents {
        o.extend_from_slice(&p.0);
    }
    put_str(o, &v.lineage.source);
    put_str(o, &v.lineage.operation);
    o.extend_from_slice(&(v.lineage.inputs.len() as u32).to_le_bytes());
    for i in &v.lineage.inputs {
        o.extend_from_slice(&i.0);
    }
    o.extend_from_slice(&(v.row_count as u64).to_le_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], StoreError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| StoreError::Io("codec truncated".into()))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u32(&mut self) -> Result<usize, StoreError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn u64(&mut self) -> Result<u64, StoreError> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    fn arr32(&mut self) -> Result<[u8; 32], StoreError> {
        let mut a = [0u8; 32];
        a.copy_from_slice(self.take(32)?);
        Ok(a)
    }
    fn cid(&mut self) -> Result<ContentId, StoreError> {
        Ok(ContentId(self.arr32()?))
    }
    fn string(&mut self) -> Result<String, StoreError> {
        let n = self.u32()?;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| StoreError::Io("codec invalid utf8".into()))
    }
}

fn decode_version(b: &[u8]) -> Result<DatasetVersion, StoreError> {
    let mut r = Reader { b, pos: 0 };
    let content_id = r.cid()?;
    let schema_hash = r.arr32()?;
    let nparents = r.u32()?;
    let mut parents = Vec::with_capacity(nparents);
    for _ in 0..nparents {
        parents.push(r.cid()?);
    }
    let source = r.string()?;
    let operation = r.string()?;
    let ninputs = r.u32()?;
    let mut inputs = Vec::with_capacity(ninputs);
    for _ in 0..ninputs {
        inputs.push(r.cid()?);
    }
    let row_count = r.u64()? as usize;
    if r.pos != b.len() {
        return Err(StoreError::Io("version codec trailing bytes".into()));
    }
    Ok(DatasetVersion {
        content_id,
        schema_hash,
        parents,
        lineage: Lineage {
            source,
            operation,
            inputs,
        },
        row_count,
    })
}

// --- hex (filenames + ref content) ---------------------------------------

fn hex(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(LUT[(b >> 4) as usize] as char);
        s.push(LUT[(b & 0x0f) as usize] as char);
    }
    s
}

fn hex_decode(s: &str) -> Result<Vec<u8>, StoreError> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err(StoreError::Io("odd-length hex".into()));
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char)
            .to_digit(16)
            .ok_or_else(|| StoreError::Io("bad hex".into()))?;
        let lo = (b[i + 1] as char)
            .to_digit(16)
            .ok_or_else(|| StoreError::Io("bad hex".into()))?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Ok(out)
}

fn content_id_from_hex(s: &str) -> Result<ContentId, StoreError> {
    let v = hex_decode(s)?;
    let arr: [u8; 32] = v
        .try_into()
        .map_err(|_| StoreError::Io("content id not 32 bytes".into()))?;
    Ok(ContentId(arr))
}
