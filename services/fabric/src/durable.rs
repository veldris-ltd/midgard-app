//! The durable, at-rest-encrypted [`DurableObjectStore`]. [DD-1]
//!
//! Persistence is a **KMS-sealed, append-only write-ahead journal** of store
//! mutations (`PutObject` / `PutLink`), each framed, encrypted at rest (via a
//! [`Sealer`] — the production impl is `kms::KmsSealer`, AES-256-GCM under
//! ML-KEM-1024), `fsync`ed, and written with `0600` perms. On [`DurableObjectStore::open`]
//! the journal is decrypted and **replayed** into an in-memory [`InMemoryStore`],
//! which rebuilds the type + inverted-property indexes and adjacency incrementally
//! — so after a restart every object, link, and index is recovered and queries
//! return identical results. A tampered on-disk record fails AEAD authentication
//! and the open fails closed. [SAD_00 §8.3] [RC-10]

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use audit_writer::Sealer;
use policy::{ClassificationLevel, Compartment, Marking, Purpose};

use crate::error::FabricError;
use crate::object::{LinkInstance, ObjectInstance};
use crate::store::{InMemoryStore, ObjectStore};
use crate::value::Value;

const JOURNAL_DS: &[u8] = b"midgard-fabric-store-v1";
const OP_PUT_OBJECT: u8 = 0;
const OP_PUT_LINK: u8 = 1;

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
        })
    }

    /// Durably insert/replace an object, returning a typed error on a persistence
    /// failure (the fallible counterpart of the infallible [`ObjectStore`] trait
    /// method). On error nothing is applied to memory, so disk and memory stay
    /// consistent.
    pub fn put_object_durable(&mut self, obj: ObjectInstance) -> Result<(), FabricError> {
        let mut bytes = vec![OP_PUT_OBJECT];
        encode_object(&mut bytes, &obj);
        self.journal(&bytes)?;
        self.inner.put_object(obj);
        Ok(())
    }

    /// Durably insert a link. See [`DurableObjectStore::put_object_durable`].
    pub fn put_link_durable(&mut self, link: LinkInstance) -> Result<(), FabricError> {
        let mut bytes = vec![OP_PUT_LINK];
        encode_link(&mut bytes, &link);
        self.journal(&bytes)?;
        self.inner.put_link(link);
        Ok(())
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

    /// Seal + frame + fsync one op's plaintext, advancing the sequence.
    fn journal(&mut self, plaintext: &[u8]) -> Result<(), FabricError> {
        let sealed = self
            .sealer
            .seal(&aad(&self.store_id, self.op_seq), plaintext)
            .map_err(|e| FabricError::Store(e.to_string()))?;
        let mut frame = Vec::with_capacity(4 + sealed.len());
        frame.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
        frame.extend_from_slice(&sealed);
        self.file.write_all(&frame).map_err(io)?;
        self.file.sync_all().map_err(io)?;
        self.op_seq += 1;
        Ok(())
    }
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

enum Op {
    PutObject(ObjectInstance),
    PutLink(LinkInstance),
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
        t => return Err(FabricError::Store(format!("unknown op tag {t}"))),
    };
    if r.pos != b.len() {
        return Err(FabricError::Store("codec trailing bytes".into()));
    }
    Ok(op)
}
