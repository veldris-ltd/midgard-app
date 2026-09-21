//! Reversible byte codec for persisting a [`SealedEntry`] to a durable ledger.
//!
//! Distinct from [`crate::canonical`], which is a one-way *hashing preimage* (it
//! carries a domain tag and is never decoded). This codec round-trips every field
//! — record, sequence, chain hashes, signature algorithm, and PQ signature — so a
//! [`crate::file::FileLedger`] can reload the exact entries after a restart. All
//! fields are length-prefixed (injective); no serde. [NFR-PQ-3]

use pqc_agility::{SigId, Signature};

use crate::error::AuditError;
use crate::record::{Action, Actor, AuditRecord, RecordId, Resource, SealedEntry, Timestamp};

const ENTRY_SCHEMA: u8 = 1;

fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

/// Encode a sealed entry to its canonical, decodable byte form.
pub fn encode_sealed_entry(e: &SealedEntry) -> Vec<u8> {
    let mut o = Vec::new();
    o.push(ENTRY_SCHEMA);
    o.extend_from_slice(&e.seq.to_le_bytes());
    o.extend_from_slice(&e.prev_hash);
    o.extend_from_slice(&e.entry_hash);
    o.push(e.sig_alg.as_u8());
    put_bytes(&mut o, e.pq_signature.as_bytes());
    put_bytes(&mut o, e.record.id.0.as_bytes());
    put_bytes(&mut o, e.record.actor.0.as_bytes());
    put_bytes(&mut o, e.record.action.0.as_bytes());
    put_bytes(&mut o, e.record.resource.0.as_bytes());
    put_bytes(&mut o, e.record.ts.0.as_bytes());
    o
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], AuditError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| AuditError::Store("codec length overflow".into()))?;
        if end > self.b.len() {
            return Err(AuditError::Store("codec truncated".into()));
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, AuditError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<usize, AuditError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn u64(&mut self) -> Result<u64, AuditError> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    fn arr32(&mut self) -> Result<[u8; 32], AuditError> {
        let mut a = [0u8; 32];
        a.copy_from_slice(self.take(32)?);
        Ok(a)
    }
    fn bytes(&mut self) -> Result<Vec<u8>, AuditError> {
        let n = self.u32()?;
        Ok(self.take(n)?.to_vec())
    }
    fn string(&mut self) -> Result<String, AuditError> {
        String::from_utf8(self.bytes()?).map_err(|_| AuditError::Store("codec invalid utf8".into()))
    }
}

/// Decode a sealed entry. Fails closed on a schema mismatch, truncation, trailing
/// bytes, or an unknown signature algorithm.
pub fn decode_sealed_entry(b: &[u8]) -> Result<SealedEntry, AuditError> {
    let mut r = Reader::new(b);
    let schema = r.u8()?;
    if schema != ENTRY_SCHEMA {
        return Err(AuditError::Store(format!("unknown entry schema {schema}")));
    }
    let seq = r.u64()?;
    let prev_hash = r.arr32()?;
    let entry_hash = r.arr32()?;
    let sig_alg =
        SigId::from_u8(r.u8()?).ok_or_else(|| AuditError::Store("unknown sig algorithm".into()))?;
    let sig_bytes = r.bytes()?;
    let id = r.string()?;
    let actor = r.string()?;
    let action = r.string()?;
    let resource = r.string()?;
    let ts = r.string()?;
    if r.pos != b.len() {
        return Err(AuditError::Store("codec trailing bytes".into()));
    }
    Ok(SealedEntry {
        record: AuditRecord {
            id: RecordId(id),
            actor: Actor(actor),
            action: Action(action),
            resource: Resource(resource),
            ts: Timestamp(ts),
        },
        seq,
        prev_hash,
        entry_hash,
        sig_alg,
        pq_signature: Signature::new(sig_alg, sig_bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_sealed_entry() {
        let e = SealedEntry {
            record: AuditRecord {
                id: RecordId("r1".into()),
                actor: Actor("alice".into()),
                action: Action("PERMIT read".into()),
                resource: Resource("obj/42".into()),
                ts: Timestamp("2026-07-24T00:00:00Z".into()),
            },
            seq: 7,
            prev_hash: [9u8; 32],
            entry_hash: [3u8; 32],
            sig_alg: SigId::MlDsa65,
            pq_signature: Signature::new(SigId::MlDsa65, vec![1, 2, 3, 4, 5]),
        };
        let bytes = encode_sealed_entry(&e);
        assert_eq!(decode_sealed_entry(&bytes).unwrap(), e);
        // Truncation and trailing-byte corruption both fail closed.
        assert!(decode_sealed_entry(&bytes[..bytes.len() - 1]).is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode_sealed_entry(&extra).is_err());
    }
}
