//! The durable, at-rest-encrypted [`DurableObjectStore`]. [DD-1]
//!
//! Persistence is a **KMS-sealed, append-only write-ahead journal** of store
//! mutations (`PutObject` / `PutLink` / `RemoveObject`), each framed, encrypted at rest (via a
//! [`Sealer`] — the production impl is `kms::KmsSealer`, AES-256-GCM under
//! ML-KEM-1024), `fsync`ed before the write returns, and written with `0600` perms. On
//! [`DurableObjectStore::open`] the journal is decrypted and **replayed** into an in-memory [`InMemoryStore`],
//! which rebuilds the type + inverted-property indexes and adjacency incrementally
//! — so after a restart every object, link, and index is recovered and queries
//! return identical results. A tampered on-disk record fails AEAD authentication
//! and the open fails closed. [SAD_00 §8.3] [RC-10]
//!
//! [`DurableObjectStore::apply_batch`] makes many mutations durable with one flush
//! (on macOS a flush is a full `F_FULLFSYNC`, several milliseconds, so one per record
//! stalls a large sync). The journal is the same: one sealed frame per record. A
//! crash before a batch's flush leaves either a prefix of the journal — whole records,
//! which replay to a consistent state — or a torn journal (a power loss can persist
//! an unflushed batch's bytes out of order, leaving a cut or a hole), and a torn
//! journal fails the open closed: nothing acknowledged is lost, and nothing
//! unacknowledged is half-applied.
//!
//! A write that fails is cut back off the journal. If even that fails the store is
//! **wedged**: every later write is refused until [`DurableObjectStore::compact`]
//! rewrites the journal from memory (which never took the failed write), the
//! recovery path.
//!
//! Every update appends a whole object, so the journal outgrows the data it holds.
//! [`DurableObjectStore::compact`] rewrites it as one `PutObject` per object and one
//! `PutLink` per link, sealed afresh, and swaps it in with an atomic rename, so a
//! crash leaves the old journal or the new one, never a mix.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use audit_writer::Sealer;
use policy::{ClassificationLevel, Compartment, Marking, Purpose};

use crate::error::FabricError;
use crate::object::{LinkInstance, ObjectInstance};
use crate::store::{InMemoryStore, ObjectStore, StoreOp};
use crate::value::Value;

const JOURNAL_DS: &[u8] = b"midgard-fabric-store-v1";
const OP_PUT_OBJECT: u8 = 0;
const OP_PUT_LINK: u8 = 1;
// Added with retention: a journal holding one cannot be read by an older build.
const OP_REMOVE_OBJECT: u8 = 2;

/// A durable object store: an [`InMemoryStore`] for reads/indexes, backed by an
/// encrypted append-only journal that makes every write survive a restart.
pub struct DurableObjectStore {
    inner: InMemoryStore,
    file: File,
    path: PathBuf,
    sealer: Box<dyn Sealer>,
    store_id: String,
    op_seq: u64,
    poison: Option<String>,
    wedged: Option<String>,
    flushes: u64,
    #[cfg(test)]
    faults: Vec<Fault>,
}

/// An injected journal I/O failure, for tests of the failure paths.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    /// Write half the frames, then fail.
    WriteHalf,
    /// Write every frame, then fail the flush.
    Sync,
    /// Fail cutting a failed write back off the journal.
    Cut,
}

impl DurableObjectStore {
    /// Open (creating if absent) the store journal at `path`, decrypting and
    /// replaying every mutation into memory. Fails closed if any on-disk record
    /// cannot be authenticated or decoded (at-rest tamper/corruption).
    pub fn open(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        store_id: impl Into<String>,
    ) -> Result<Self, FabricError> {
        let path = path.as_ref().to_path_buf();
        let store_id = store_id.into();
        let mut inner = InMemoryStore::new();
        let mut op_seq = 0u64;

        if path.exists() {
            let mut buf = Vec::new();
            File::open(&path)
                .map_err(io)?
                .read_to_end(&mut buf)
                .map_err(io)?;
            let mut pos = 0usize;
            while pos < buf.len() {
                if pos + 4 > buf.len() {
                    return Err(FabricError::Store("truncated frame header".into()));
                }
                let len = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
                    as usize;
                pos += 4;
                let end = pos
                    .checked_add(len)
                    .ok_or_else(|| FabricError::Store("frame length overflow".into()))?;
                if end > buf.len() {
                    return Err(FabricError::Store("truncated frame body".into()));
                }
                let plaintext = sealer
                    .unseal(&aad(&store_id, op_seq), &buf[pos..end])
                    .map_err(|e| FabricError::Store(e.to_string()))?;
                pos = end;
                match decode_op(&plaintext)? {
                    Op::PutObject(o) => inner.put_object(o),
                    Op::PutLink(l) => inner.put_link(l),
                    Op::RemoveObject(id) => {
                        inner.remove_object(&id);
                    }
                }
                op_seq += 1;
            }
        }

        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&path).map_err(io)?;

        Ok(Self {
            inner,
            file,
            path,
            sealer,
            store_id,
            op_seq,
            poison: None,
            wedged: None,
            flushes: 0,
            #[cfg(test)]
            faults: Vec::new(),
        })
    }

    /// Durably insert/replace an object, returning a typed error on a persistence
    /// failure (the fallible counterpart of the infallible [`ObjectStore`] trait
    /// method). On error nothing is applied to memory, so disk and memory stay
    /// consistent.
    pub fn put_object_durable(&mut self, obj: ObjectInstance) -> Result<(), FabricError> {
        self.apply_batch(vec![StoreOp::PutObject(obj)]).map(drop)
    }

    /// Durably insert a link. See [`DurableObjectStore::put_object_durable`].
    pub fn put_link_durable(&mut self, link: LinkInstance) -> Result<(), FabricError> {
        self.apply_batch(vec![StoreOp::PutLink(link)]).map(drop)
    }

    /// Durably remove an object and its links, returning whether there was one (nothing
    /// is journalled when there was not). See [`DurableObjectStore::put_object_durable`].
    pub fn remove_object_durable(&mut self, id: &str) -> Result<bool, FabricError> {
        self.apply_batch(vec![StoreOp::RemoveObject(id.to_string())])
            .map(|written| written == 1)
    }

    /// Durably apply `ops`, in order, with **one** flush, returning how many records
    /// were journalled (a removal of an object that is not there at that point writes
    /// none). Every record is sealed and framed exactly as a single write's is, all are
    /// appended, the journal is flushed once, and only then are they applied to memory:
    /// so when this returns `Ok` every op is on disk, and on `Err` none is applied and
    /// the journal is cut back to where it was.
    ///
    /// A wedged store (see [`DurableObjectStore::wedged`]) refuses every batch.
    pub fn apply_batch(&mut self, ops: Vec<StoreOp>) -> Result<usize, FabricError> {
        if let Some(why) = &self.wedged {
            return Err(FabricError::Store(format!("journal unusable: {why}")));
        }
        // Which removals find an object, given the ops before them in the batch.
        let mut present: std::collections::BTreeMap<String, bool> = Default::default();
        let mut kept = Vec::with_capacity(ops.len());
        for op in ops {
            match &op {
                StoreOp::PutObject(o) => {
                    present.insert(o.id.clone(), true);
                }
                StoreOp::PutLink(_) => {}
                StoreOp::RemoveObject(id) => {
                    let there = match present.get(id) {
                        Some(there) => *there,
                        None => self.inner.get_object(id).is_some(),
                    };
                    if !there {
                        continue;
                    }
                    present.insert(id.clone(), false);
                }
            }
            kept.push(op);
        }
        if kept.is_empty() {
            return Ok(0);
        }

        let mut frames = Vec::new();
        for (i, op) in kept.iter().enumerate() {
            let sealed = self
                .sealer
                .seal(&aad(&self.store_id, self.op_seq + i as u64), &encode_op(op))
                .map_err(|e| FabricError::Store(e.to_string()))?;
            frames.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
            frames.extend_from_slice(&sealed);
        }
        self.append(&frames)?;
        self.op_seq += kept.len() as u64;

        let written = kept.len();
        for op in kept {
            match op {
                StoreOp::PutObject(o) => self.inner.put_object(o),
                StoreOp::PutLink(l) => self.inner.put_link(l),
                StoreOp::RemoveObject(id) => {
                    self.inner.remove_object(&id);
                }
            }
        }
        Ok(written)
    }

    /// Why the store refuses writes, if a failed write could not be cut back off the
    /// journal: the journal may then hold records memory does not, and the next record
    /// would reuse a sequence number. `None` is healthy. A successful
    /// [`DurableObjectStore::compact`] rewrites the journal from memory and clears it.
    pub fn wedged(&self) -> Option<&str> {
        self.wedged.as_deref()
    }

    /// Wedge the store as a failed write that could not be cut back would, for a
    /// dependant's tests of recovery (`test-faults` feature only).
    #[cfg(feature = "test-faults")]
    #[doc(hidden)]
    pub fn wedge(&mut self, why: &str) {
        self.wedged = Some(why.to_string());
    }

    /// How many times the journal has been flushed to disk since it was opened.
    pub fn flushes(&self) -> u64 {
        self.flushes
    }

    /// Journal records written since the journal began (or was last compacted).
    pub fn journal_records(&self) -> u64 {
        self.op_seq
    }

    /// Records a compacted journal would hold: one per object and one per link.
    pub fn live_records(&self) -> u64 {
        let (objects, links) = self.inner.len();
        (objects + links) as u64
    }

    /// Rewrite the journal to hold only what the store holds now, returning the record
    /// counts before and after. Written beside the journal, fsynced, then renamed over
    /// it; on any error the old journal is untouched and still in use. Success also
    /// un-wedges the store (see [`DurableObjectStore::wedged`]): the new journal holds
    /// exactly what memory does.
    pub fn compact(&mut self) -> Result<(u64, u64), FabricError> {
        let before = self.op_seq;
        let tmp = self.path.with_extension("compacting");
        let mut opts = OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut out = opts.open(&tmp).map_err(io)?;
        let mut seq = 0u64;
        let mut write = |plaintext: &[u8], seq: &mut u64| -> Result<(), FabricError> {
            let sealed = self
                .sealer
                .seal(&aad(&self.store_id, *seq), plaintext)
                .map_err(|e| FabricError::Store(e.to_string()))?;
            out.write_all(&(sealed.len() as u32).to_le_bytes())
                .map_err(io)?;
            out.write_all(&sealed).map_err(io)?;
            *seq += 1;
            Ok(())
        };
        let written = (|| {
            for obj in self.inner.objects() {
                let mut bytes = vec![OP_PUT_OBJECT];
                encode_object(&mut bytes, obj);
                write(&bytes, &mut seq)?;
            }
            for link in self.inner.links() {
                let mut bytes = vec![OP_PUT_LINK];
                encode_link(&mut bytes, &link);
                write(&bytes, &mut seq)?;
            }
            Ok::<(), FabricError>(())
        })();
        let synced = written.and_then(|()| out.sync_all().map_err(io));
        drop(out);
        if let Err(e) = synced {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        std::fs::rename(&tmp, &self.path).map_err(io)?;
        if let Some(dir) = self.path.parent() {
            if let Ok(d) = File::open(dir) {
                let _ = d.sync_all();
            }
        }
        self.file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(io)?;
        self.op_seq = seq;
        self.wedged = None;
        Ok((before, seq))
    }

    /// The latched persistence error, if a prior infallible write failed to
    /// durably commit (the infallible trait cannot return it). `None` is healthy.
    pub fn poisoned(&self) -> Option<&str> {
        self.poison.as_deref()
    }

    /// The journal file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append sealed frames to the journal and flush it. On failure the journal is cut
    /// back to its old length: a record left on disk but not in memory would come back
    /// after a restart, and the next write would reuse its sequence number. If it
    /// cannot be cut back, the store is wedged.
    fn append(&mut self, frames: &[u8]) -> Result<(), FabricError> {
        let start = self.file.metadata().map_err(io)?.len();
        let done = self.write_frames(frames).and_then(|()| self.flush());
        if let Err(e) = done {
            if let Err(cut) = self.cut_back(start) {
                self.wedged = Some(format!(
                    "a failed write ({e}) could not be cut back off the journal ({cut}); \
                     compact the store to recover"
                ));
            }
            return Err(io(e));
        }
        self.flushes += 1;
        Ok(())
    }

    fn write_frames(&mut self, frames: &[u8]) -> std::io::Result<()> {
        #[cfg(test)]
        if self.inject(Fault::WriteHalf) {
            self.file.write_all(&frames[..frames.len() / 2])?;
            return Err(injected());
        }
        self.file.write_all(frames)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        #[cfg(test)]
        if self.inject(Fault::Sync) {
            return Err(injected());
        }
        self.file.sync_all()
    }

    fn cut_back(&mut self, len: u64) -> std::io::Result<()> {
        #[cfg(test)]
        if self.inject(Fault::Cut) {
            return Err(injected());
        }
        self.file.set_len(len)?;
        self.file.sync_all()
    }

    /// Take `fault` if it is armed.
    #[cfg(test)]
    fn inject(&mut self, fault: Fault) -> bool {
        match self.faults.iter().position(|f| *f == fault) {
            Some(i) => {
                self.faults.remove(i);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
fn injected() -> std::io::Error {
    std::io::Error::other("injected fault")
}

impl ObjectStore for DurableObjectStore {
    fn put_object(&mut self, obj: ObjectInstance) {
        // The trait is infallible; latch a persistence failure for `poisoned()`.
        // On failure the write is rejected entirely (not applied to memory).
        if let Err(e) = self.put_object_durable(obj) {
            self.poison = Some(e.to_string());
        }
    }
    fn get_object(&self, id: &str) -> Option<&ObjectInstance> {
        self.inner.get_object(id)
    }
    fn objects_of_type(&self, type_id: &str) -> Vec<&ObjectInstance> {
        self.inner.objects_of_type(type_id)
    }
    fn by_property(&self, type_id: &str, property: &str, key: &str) -> Vec<&ObjectInstance> {
        self.inner.by_property(type_id, property, key)
    }
    fn put_link(&mut self, link: LinkInstance) {
        if let Err(e) = self.put_link_durable(link) {
            self.poison = Some(e.to_string());
        }
    }
    fn neighbors(&self, source_id: &str, link_type: &str) -> Vec<&ObjectInstance> {
        self.inner.neighbors(source_id, link_type)
    }
    fn try_apply_all(&mut self, ops: Vec<StoreOp>) -> Result<(), FabricError> {
        self.apply_batch(ops).map(drop)
    }
    fn remove_object(&mut self, id: &str) -> Option<ObjectInstance> {
        let obj = self.inner.get_object(id).cloned()?;
        match self.remove_object_durable(id) {
            Ok(_) => Some(obj),
            Err(e) => {
                self.poison = Some(e.to_string());
                None
            }
        }
    }
}

fn io(e: std::io::Error) -> FabricError {
    FabricError::Store(e.to_string())
}

/// AAD binding an op to its store and journal position (anti-splice/anti-reorder).
fn aad(store_id: &str, seq: u64) -> Vec<u8> {
    let mut a = Vec::with_capacity(JOURNAL_DS.len() + 4 + store_id.len() + 8);
    a.extend_from_slice(JOURNAL_DS);
    a.extend_from_slice(&(store_id.len() as u32).to_le_bytes());
    a.extend_from_slice(store_id.as_bytes());
    a.extend_from_slice(&seq.to_le_bytes());
    a
}

// --- codec (no serde; length-prefixed, injective) -------------------------

fn encode_op(op: &StoreOp) -> Vec<u8> {
    match op {
        StoreOp::PutObject(obj) => {
            let mut bytes = vec![OP_PUT_OBJECT];
            encode_object(&mut bytes, obj);
            bytes
        }
        StoreOp::PutLink(link) => {
            let mut bytes = vec![OP_PUT_LINK];
            encode_link(&mut bytes, link);
            bytes
        }
        StoreOp::RemoveObject(id) => {
            let mut bytes = vec![OP_REMOVE_OBJECT];
            put_str(&mut bytes, id);
            bytes
        }
    }
}

enum Op {
    PutObject(ObjectInstance),
    PutLink(LinkInstance),
    RemoveObject(String),
}

fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

fn encode_value(o: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Text(s) => {
            o.push(0);
            put_str(o, s);
        }
        Value::Int(i) => {
            o.push(1);
            o.extend_from_slice(&i.to_le_bytes());
        }
        Value::Float(x) => {
            o.push(2);
            o.extend_from_slice(&x.to_bits().to_le_bytes());
        }
        Value::Bool(b) => {
            o.push(3);
            o.push(*b as u8);
        }
        Value::Timestamp(t) => {
            o.push(4);
            o.extend_from_slice(&t.to_le_bytes());
        }
    }
}

fn encode_object(o: &mut Vec<u8>, obj: &ObjectInstance) {
    put_str(o, &obj.id);
    put_str(o, &obj.type_id);
    o.extend_from_slice(&(obj.properties.len() as u32).to_le_bytes());
    for (k, v) in &obj.properties {
        put_str(o, k);
        encode_value(o, v);
    }
    o.push(obj.marking.level.0);
    o.extend_from_slice(&(obj.marking.compartments.len() as u32).to_le_bytes());
    for c in &obj.marking.compartments {
        put_str(o, &c.0);
    }
    o.extend_from_slice(&(obj.allowed_purposes.len() as u32).to_le_bytes());
    for p in &obj.allowed_purposes {
        put_str(o, &p.0);
    }
}

fn encode_link(o: &mut Vec<u8>, l: &LinkInstance) {
    put_str(o, &l.link_type);
    put_str(o, &l.source_id);
    put_str(o, &l.target_id);
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], FabricError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| FabricError::Store("codec overflow".into()))?;
        if end > self.b.len() {
            return Err(FabricError::Store("codec truncated".into()));
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, FabricError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<usize, FabricError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn u64(&mut self) -> Result<u64, FabricError> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    fn i64(&mut self) -> Result<i64, FabricError> {
        Ok(self.u64()? as i64)
    }
    fn string(&mut self) -> Result<String, FabricError> {
        let n = self.u32()?;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| FabricError::Store("codec invalid utf8".into()))
    }
}

fn decode_value(r: &mut Reader) -> Result<Value, FabricError> {
    Ok(match r.u8()? {
        0 => Value::Text(r.string()?),
        1 => Value::Int(r.i64()?),
        2 => Value::Float(f64::from_bits(r.u64()?)),
        3 => Value::Bool(r.u8()? != 0),
        4 => Value::Timestamp(r.u64()?),
        t => return Err(FabricError::Store(format!("unknown value tag {t}"))),
    })
}

fn decode_object(r: &mut Reader) -> Result<ObjectInstance, FabricError> {
    let id = r.string()?;
    let type_id = r.string()?;
    let nprops = r.u32()?;
    let mut properties = std::collections::BTreeMap::new();
    for _ in 0..nprops {
        let k = r.string()?;
        properties.insert(k, decode_value(r)?);
    }
    let level = ClassificationLevel(r.u8()?);
    let ncomp = r.u32()?;
    let mut compartments = std::collections::BTreeSet::new();
    for _ in 0..ncomp {
        compartments.insert(Compartment(r.string()?));
    }
    let npurp = r.u32()?;
    let mut allowed_purposes = std::collections::BTreeSet::new();
    for _ in 0..npurp {
        allowed_purposes.insert(Purpose(r.string()?));
    }
    Ok(ObjectInstance {
        id,
        type_id,
        properties,
        marking: Marking {
            level,
            compartments,
        },
        allowed_purposes,
    })
}

fn decode_op(b: &[u8]) -> Result<Op, FabricError> {
    let mut r = Reader { b, pos: 0 };
    let op = match r.u8()? {
        OP_PUT_OBJECT => Op::PutObject(decode_object(&mut r)?),
        OP_PUT_LINK => Op::PutLink(LinkInstance {
            link_type: r.string()?,
            source_id: r.string()?,
            target_id: r.string()?,
        }),
        OP_REMOVE_OBJECT => Op::RemoveObject(r.string()?),
        t => return Err(FabricError::Store(format!("unknown op tag {t}"))),
    };
    if r.pos != b.len() {
        return Err(FabricError::Store("codec trailing bytes".into()));
    }
    Ok(op)
}

#[cfg(test)]
mod tests {
    //! The journal's I/O failure paths, driven by injected faults.

    use super::*;
    use audit_writer::AuditError;
    use std::collections::{BTreeMap, BTreeSet};

    /// Seals nothing: these tests are about the journal file, not encryption.
    struct Plain;
    impl Sealer for Plain {
        fn seal(&self, _aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
            Ok(plaintext.to_vec())
        }
        fn unseal(&self, _aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
            Ok(sealed.to_vec())
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let uniq = Box::into_raw(Box::new(0u8)) as usize;
        let dir = std::env::temp_dir().join(format!(
            "fabric-durable-unit-{tag}-{}-{uniq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("fabric.store")
    }

    fn open(path: &Path) -> DurableObjectStore {
        DurableObjectStore::open(path, Box::new(Plain), "unit").unwrap()
    }

    fn obj(id: &str) -> ObjectInstance {
        ObjectInstance {
            id: id.into(),
            type_id: "track".into(),
            properties: BTreeMap::new(),
            marking: Marking {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            allowed_purposes: BTreeSet::new(),
        }
    }

    fn batch(ids: &[&str]) -> Vec<StoreOp> {
        ids.iter().map(|id| StoreOp::PutObject(obj(id))).collect()
    }

    fn len(path: &Path) -> u64 {
        std::fs::metadata(path).unwrap().len()
    }

    fn ids(store: &DurableObjectStore) -> Vec<String> {
        store
            .objects_of_type("track")
            .iter()
            .map(|o| o.id.clone())
            .collect()
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A batch whose write or flush fails leaves the journal, its record count, the
    /// flush count and memory exactly as they were, and the store carries on.
    fn a_failed_append_is_taken_back(fault: Fault) {
        let path = scratch("cut");
        {
            let mut store = open(&path);
            store.put_object_durable(obj("a")).unwrap();
            let (bytes, records, flushes) = (len(&path), store.journal_records(), store.flushes());
            store.faults.push(fault);
            assert!(store.apply_batch(batch(&["b", "c", "d"])).is_err());
            assert_eq!(
                len(&path),
                bytes,
                "the partial batch was cut off the journal"
            );
            assert_eq!(store.journal_records(), records);
            assert_eq!(store.flushes(), flushes, "only successful flushes count");
            assert_eq!(ids(&store), vec!["a"], "memory unchanged");
            assert!(store.wedged().is_none());
            store.apply_batch(batch(&["e"])).unwrap();
        }
        assert_eq!(
            ids(&open(&path)),
            vec!["a", "e"],
            "reopens with what succeeded"
        );
        cleanup(&path);
    }

    #[test]
    fn a_short_write_is_taken_back() {
        a_failed_append_is_taken_back(Fault::WriteHalf);
    }

    #[test]
    fn a_failed_flush_is_taken_back() {
        a_failed_append_is_taken_back(Fault::Sync);
    }

    #[test]
    fn a_journal_that_cannot_be_taken_back_wedges_the_store_until_it_is_compacted() {
        let path = scratch("wedge");
        {
            let mut store = open(&path);
            store.put_object_durable(obj("a")).unwrap();
            // The frames reach the file, the flush fails, and so does cutting them off.
            store.faults.extend([Fault::Sync, Fault::Cut]);
            assert!(store.apply_batch(batch(&["b", "c"])).is_err());
            assert!(store.wedged().is_some());
            assert_eq!(ids(&store), vec!["a"]);

            for refused in [
                store.apply_batch(batch(&["d"])).unwrap_err(),
                store.put_object_durable(obj("d")).unwrap_err(),
                store.remove_object_durable("a").unwrap_err(),
            ] {
                assert!(
                    refused.to_string().contains("journal unusable"),
                    "{refused}"
                );
            }
            assert_eq!(ids(&store), vec!["a"]);

            // Compaction rewrites the journal from memory: the recovery path.
            store.compact().unwrap();
            assert!(store.wedged().is_none());
            store.apply_batch(batch(&["e"])).unwrap();
        }
        assert_eq!(
            ids(&open(&path)),
            vec!["a", "e"],
            "the stray frames went with the old journal"
        );
        cleanup(&path);
    }

    #[test]
    fn a_journal_handle_that_cannot_be_written_wedges_the_store() {
        // A real I/O failure, no injection: a read-only handle refuses the write and
        // the cut-back alike.
        let path = scratch("readonly");
        {
            let mut store = open(&path);
            store.put_object_durable(obj("a")).unwrap();
            let bytes = len(&path);
            store.file = File::open(&path).unwrap();
            assert!(store.apply_batch(batch(&["b"])).is_err());
            assert!(store.wedged().is_some());
            assert_eq!(len(&path), bytes);
            assert_eq!(store.journal_records(), 1);
            assert_eq!(ids(&store), vec!["a"]);
            store.compact().unwrap();
            store.apply_batch(batch(&["c"])).unwrap();
        }
        assert_eq!(ids(&open(&path)), vec!["a", "c"]);
        cleanup(&path);
    }
}
