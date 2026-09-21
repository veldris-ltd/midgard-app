//! # midgard-client-sync — offline-first CRDT sync
//!
//! Every client works disconnected: an edit is a local **CRDT operation** applied
//! immediately and queued; on reconnect the queued ops are pushed and remote ops
//! merged, and the merge is **conflict-free** — commutative, associative, and
//! idempotent — so every replica converges to byte-identical state with no data
//! loss regardless of the order ops arrive. `[FR-C-4]` `[AC-C-4]`
//!
//! The state is a **last-writer-wins map**: an object is a set of fields, each a
//! register carrying its value and a [`Version`]. A version is a Lamport clock
//! paired with the replica id, giving a **total order** with a deterministic
//! tie-break — so two replicas that have seen the same ops resolve every conflict
//! identically without coordination.
//!
//! - Concurrent edits to *different* fields both survive (no loss).
//! - Concurrent edits to the *same* field resolve to the higher `(lamport,
//!   replica)` — the standard LWW rule, applied identically everywhere.
//! - A delete is a tombstone (a register with no value), itself LWW-ordered, so a
//!   later re-create on any replica wins over an earlier delete.
//!
//! This crate is pure logic with **zero dependencies** and no crypto: ops are
//! sealed in transit and at rest by the layers above (`midgard-client-store`,
//! `midgard-client-enclosure`). `[KDD-1]`

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

mod error;
pub use error::SyncError;

/// An object identifier in the local cache.
pub type ObjectId = String;
/// A field name within an object.
pub type Field = String;

/// A version stamp: a Lamport clock plus the originating replica id. Ordered by
/// `(lamport, replica)` — a **total order** so every replica breaks ties the same
/// way. Two distinct edits never share a version (a replica bumps its clock on
/// each edit), so equal versions imply the identical op → idempotent merges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    /// Lamport logical clock at the time of the edit.
    pub lamport: u64,
    /// The replica that produced the edit.
    pub replica: String,
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport
            .cmp(&other.lamport)
            .then_with(|| self.replica.cmp(&other.replica))
    }
}

/// A single conflict-free operation. Self-contained: it carries everything the
/// merge needs, so ops can be applied in any order on any replica. A `value` of
/// `None` is a delete (tombstone).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncOp {
    /// Target object.
    pub object: ObjectId,
    /// Target field.
    pub field: Field,
    /// New value, or `None` to delete the field.
    pub value: Option<Vec<u8>>,
    /// The version this edit carries.
    pub version: Version,
}

impl SyncOp {
    /// Canonical, length-prefixed byte encoding — an op the shell can treat as an
    /// **opaque blob** to hand across the FFI binding and push to the enclosure
    /// relay, without ever touching the CRDT internals (KDD-1). Deterministic and
    /// dependency-free. [RT-58] [FR-C-1]
    pub fn encode(&self) -> Vec<u8> {
        fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
            o.extend_from_slice(&(b.len() as u32).to_le_bytes());
            o.extend_from_slice(b);
        }
        let mut o = Vec::new();
        put_bytes(&mut o, self.object.as_bytes());
        put_bytes(&mut o, self.field.as_bytes());
        match &self.value {
            Some(v) => {
                o.push(1);
                put_bytes(&mut o, v);
            }
            None => o.push(0), // tombstone (delete)
        }
        o.extend_from_slice(&self.version.lamport.to_le_bytes());
        put_bytes(&mut o, self.version.replica.as_bytes());
        o
    }

    /// Decode an op produced by [`encode`](SyncOp::encode). **Fails closed** on any
    /// malformed input — a truncated or oversized op is rejected, never applied.
    /// [RT-58]
    pub fn decode(bytes: &[u8]) -> Result<Self, SyncError> {
        let mut r = OpReader { b: bytes, pos: 0 };
        let object = r.string()?;
        let field = r.string()?;
        let value = match r.u8()? {
            0 => None,
            1 => Some(r.bytes()?),
            _ => return Err(SyncError::MalformedOp("bad value tag".into())),
        };
        let lamport = r.u64()?;
        let replica = r.string()?;
        r.finish()?;
        Ok(SyncOp {
            object,
            field,
            value,
            version: Version { lamport, replica },
        })
    }
}

/// A bounds-checked cursor for [`SyncOp::decode`]; every read past the end fails
/// closed with a [`SyncError::MalformedOp`].
struct OpReader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> OpReader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], SyncError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| SyncError::MalformedOp("length overflow".into()))?;
        if end > self.b.len() {
            return Err(SyncError::MalformedOp("unexpected end of op".into()));
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, SyncError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, SyncError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Result<u64, SyncError> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_le_bytes(a))
    }
    fn bytes(&mut self) -> Result<Vec<u8>, SyncError> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn string(&mut self) -> Result<String, SyncError> {
        String::from_utf8(self.bytes()?).map_err(|_| SyncError::MalformedOp("invalid utf-8".into()))
    }
    fn finish(self) -> Result<(), SyncError> {
        if self.pos == self.b.len() {
            Ok(())
        } else {
            Err(SyncError::MalformedOp("trailing bytes".into()))
        }
    }
}

/// A last-writer-wins register: the current value (or a tombstone) and the
/// version that set it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Register {
    version: Version,
    value: Option<Vec<u8>>,
}

/// One replica of the shared state: its local view, its Lamport clock, and the
/// queue of not-yet-synced ops.
#[derive(Clone, Debug)]
pub struct Replica {
    id: String,
    lamport: u64,
    state: BTreeMap<(ObjectId, Field), Register>,
    pending: Vec<SyncOp>,
}

impl Replica {
    /// A fresh, empty replica with a stable id. The id participates in version
    /// tie-breaks, so it must be unique per device/session.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            lamport: 0,
            state: BTreeMap::new(),
            pending: Vec::new(),
        }
    }

    /// This replica's id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Set `field` on `object` to `value`, applied locally and queued for sync.
    /// Returns the op so the caller can also hand it to the store/enclosure.
    pub fn set(
        &mut self,
        object: impl Into<String>,
        field: impl Into<String>,
        value: Vec<u8>,
    ) -> SyncOp {
        self.local_edit(object.into(), field.into(), Some(value))
    }

    /// Delete `field` on `object` (a tombstone), applied locally and queued.
    pub fn delete(&mut self, object: impl Into<String>, field: impl Into<String>) -> SyncOp {
        self.local_edit(object.into(), field.into(), None)
    }

    fn local_edit(&mut self, object: ObjectId, field: Field, value: Option<Vec<u8>>) -> SyncOp {
        self.lamport += 1;
        let op = SyncOp {
            object,
            field,
            value,
            version: Version {
                lamport: self.lamport,
                replica: self.id.clone(),
            },
        };
        self.apply(&op);
        self.pending.push(op.clone());
        op
    }

    /// Merge a remote op. Idempotent, commutative, and associative: applying the
    /// same op twice, or ops in any order, yields the same state. Advances the
    /// local Lamport clock so subsequent local edits causally follow what we have
    /// seen. `[AC-C-4]`
    pub fn merge(&mut self, op: &SyncOp) {
        if op.version.lamport > self.lamport {
            self.lamport = op.version.lamport;
        }
        self.apply(op);
    }

    /// Merge many remote ops.
    pub fn merge_all<'a, I>(&mut self, ops: I)
    where
        I: IntoIterator<Item = &'a SyncOp>,
    {
        for op in ops {
            self.merge(op);
        }
    }

    /// The LWW resolution, shared by local edits and remote merges: an op wins iff
    /// its version strictly dominates the register currently present. Equal
    /// versions are the same op, so keeping the incumbent is idempotent.
    fn apply(&mut self, op: &SyncOp) {
        let key = (op.object.clone(), op.field.clone());
        let wins = match self.state.get(&key) {
            Some(existing) => op.version > existing.version,
            None => true,
        };
        if wins {
            self.state.insert(
                key,
                Register {
                    version: op.version.clone(),
                    value: op.value.clone(),
                },
            );
        }
    }

    /// The current value of a field, or `None` if absent or deleted.
    pub fn get(&self, object: &str, field: &str) -> Option<&[u8]> {
        self.state
            .get(&(object.to_string(), field.to_string()))
            .and_then(|r| r.value.as_deref())
    }

    /// A canonical snapshot of all live (non-tombstoned) fields. Two replicas that
    /// have merged the same op set produce identical snapshots — the convergence
    /// invariant. `BTreeMap` makes the snapshot deterministic.
    pub fn snapshot(&self) -> BTreeMap<(ObjectId, Field), Vec<u8>> {
        self.state
            .iter()
            .filter_map(|(k, r)| r.value.as_ref().map(|v| (k.clone(), v.clone())))
            .collect()
    }

    /// The ops queued locally and not yet acknowledged by the enclosure.
    pub fn pending(&self) -> &[SyncOp] {
        &self.pending
    }

    /// Whether there is unsynced work.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Take the queued ops to push on reconnect, clearing the queue. Call once the
    /// push is durably accepted by the enclosure relay. `[FR-C-4]`
    pub fn drain_pending(&mut self) -> Vec<SyncOp> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_edit_is_visible_and_queued() {
        let mut r = Replica::new("a");
        r.set("obj:1", "title", b"hello".to_vec());
        assert_eq!(r.get("obj:1", "title"), Some(&b"hello"[..]));
        assert_eq!(r.pending().len(), 1);
        assert!(r.has_pending());
    }

    #[test]
    fn drain_clears_the_queue_but_keeps_state() {
        let mut r = Replica::new("a");
        r.set("obj:1", "title", b"hello".to_vec());
        let drained = r.drain_pending();
        assert_eq!(drained.len(), 1);
        assert!(!r.has_pending());
        assert_eq!(r.get("obj:1", "title"), Some(&b"hello"[..]));
    }

    #[test]
    fn delete_tombstones_the_field() {
        let mut r = Replica::new("a");
        r.set("obj:1", "title", b"hello".to_vec());
        r.delete("obj:1", "title");
        assert_eq!(r.get("obj:1", "title"), None);
    }

    #[test]
    fn merge_is_idempotent() {
        let mut a = Replica::new("a");
        let op = a.set("obj:1", "title", b"x".to_vec());
        let mut b = Replica::new("b");
        b.merge(&op);
        let before = b.snapshot();
        b.merge(&op);
        b.merge(&op);
        assert_eq!(before, b.snapshot());
    }

    #[test]
    fn concurrent_same_field_resolves_by_version_deterministically() {
        // Two replicas edit the same field concurrently (same lamport). The higher
        // replica id wins on both sides — no coordination, identical outcome.
        let mut a = Replica::new("a");
        let mut b = Replica::new("b");
        let oa = a.set("obj:1", "title", b"from-a".to_vec());
        let ob = b.set("obj:1", "title", b"from-b".to_vec());
        assert_eq!(oa.version.lamport, ob.version.lamport);
        a.merge(&ob);
        b.merge(&oa);
        assert_eq!(a.snapshot(), b.snapshot());
        // "b" > "a" in the tie-break, so b's write wins on both.
        assert_eq!(a.get("obj:1", "title"), Some(&b"from-b"[..]));
    }

    #[test]
    fn concurrent_distinct_fields_both_survive() {
        let mut a = Replica::new("a");
        let mut b = Replica::new("b");
        let oa = a.set("obj:1", "title", b"t".to_vec());
        let ob = b.set("obj:1", "body", b"y".to_vec());
        a.merge(&ob);
        b.merge(&oa);
        assert_eq!(a.get("obj:1", "title"), Some(&b"t"[..]));
        assert_eq!(a.get("obj:1", "body"), Some(&b"y"[..]));
        assert_eq!(a.snapshot(), b.snapshot());
    }
}
