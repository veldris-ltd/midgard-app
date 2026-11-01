//! Reversible byte codec for persisting a [`SealedEntry`] to a durable ledger.
//!
//! Distinct from [`crate::canonical`], which is a one-way *hashing preimage* (it
//! carries a domain tag and is never decoded). This codec round-trips every field
//! — record, sequence, chain hashes, signature algorithm, and PQ signature — so a
//! [`crate::file::FileLedger`] can reload the exact entries after a restart. All
//! fields are length-prefixed (injective); no serde. [NFR-PQ-3]
//!
//! Entry schema 2 appends the record's [`AuditContext`] after the timestamp: the
//! outcome byte, then each optional field as a presence byte and, when present, its
//! length-prefixed bytes. Schema 1 entries, written before the context existed, still
//! decode, with an empty context. [VLD-ANB-MIDGARD-0052]

use pqc_agility::{SigId, Signature};

use crate::error::AuditError;
use crate::record::{
    Action, Actor, AuditContext, AuditRecord, Outcome, RecordId, Resource, SealedEntry, Timestamp,
};

const ENTRY_SCHEMA: u8 = 2;
/// The schema written before records carried an [`AuditContext`].
const ENTRY_SCHEMA_V1: u8 = 1;

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
    o.push(e.record.context.outcome.as_u8());
    for field in e.record.context.optional_fields() {
        match field {
            Some(value) => {
                o.push(1);
                put_bytes(&mut o, value.as_bytes());
            }
            None => o.push(0),
        }
    }
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
    fn optional(&mut self) -> Result<Option<String>, AuditError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.string().map(Some),
            _ => Err(AuditError::Store("codec invalid presence byte".into())),
        }
    }
    fn context(&mut self) -> Result<AuditContext, AuditError> {
        let outcome = Outcome::from_u8(self.u8()?)
            .ok_or_else(|| AuditError::Store("unknown outcome".into()))?;
        Ok(AuditContext {
            outcome,
            source: self.optional()?,
            session: self.optional()?,
            reason: self.optional()?,
            approval_id: self.optional()?,
            approver: self.optional()?,
            target: self.optional()?,
        })
    }
}

/// Decode a sealed entry, of schema 1 (an empty context) or 2. Fails closed on an
/// unknown schema, truncation, trailing bytes, or an unknown signature algorithm or
/// outcome.
pub fn decode_sealed_entry(b: &[u8]) -> Result<SealedEntry, AuditError> {
    let mut r = Reader::new(b);
    let schema = r.u8()?;
    if schema != ENTRY_SCHEMA && schema != ENTRY_SCHEMA_V1 {
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
    let context = if schema == ENTRY_SCHEMA_V1 {
        AuditContext::default()
    } else {
        r.context()?
    };
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
            context,
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
                context: AuditContext::default(),
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

    fn entry(context: AuditContext) -> SealedEntry {
        SealedEntry {
            record: AuditRecord {
                id: RecordId("AUD-3".into()),
                actor: Actor("alice".into()),
                action: Action("REFUSED Approve".into()),
                resource: Resource("aircraft/1".into()),
                ts: Timestamp("1800000000".into()),
                context,
            },
            seq: 3,
            prev_hash: [4u8; 32],
            entry_hash: [5u8; 32],
            sig_alg: SigId::MlDsa65,
            pq_signature: Signature::new(SigId::MlDsa65, vec![6, 7, 8]),
        }
    }

    /// The entry layout before the context existed, built here by hand.
    fn schema_one_bytes(e: &SealedEntry) -> Vec<u8> {
        let mut o = vec![1u8];
        o.extend_from_slice(&e.seq.to_le_bytes());
        o.extend_from_slice(&e.prev_hash);
        o.extend_from_slice(&e.entry_hash);
        o.push(e.sig_alg.as_u8());
        let r = &e.record;
        for field in [
            e.pq_signature.as_bytes(),
            r.id.0.as_bytes(),
            r.actor.0.as_bytes(),
            r.action.0.as_bytes(),
            r.resource.0.as_bytes(),
            r.ts.0.as_bytes(),
        ] {
            o.extend_from_slice(&(field.len() as u32).to_le_bytes());
            o.extend_from_slice(field);
        }
        o
    }

    /// A ledger written before the context existed still opens: its schema 1 entries
    /// decode with an empty context. [VLD-ANB-MIDGARD-0052] [AC-26.1]
    #[test]
    fn decodes_schema_one_entries_with_an_empty_context() {
        let e = entry(AuditContext::default());
        let old = schema_one_bytes(&e);
        assert_eq!(decode_sealed_entry(&old).unwrap(), e);
        // Schema 1 carries nothing after the timestamp.
        let mut extra = old.clone();
        extra.push(0);
        assert!(decode_sealed_entry(&extra).is_err());
        // An unknown schema still fails closed.
        let mut unknown = old;
        unknown[0] = 3;
        assert!(decode_sealed_entry(&unknown).is_err());
    }

    /// Every context field survives a round trip, and absent and empty fields stay
    /// distinct. [VLD-ANB-MIDGARD-0052] [AC-26.1]
    #[test]
    fn round_trips_a_populated_context() {
        let e = entry(AuditContext {
            source: Some("127.0.0.1".into()),
            session: Some("alice:1800000000".into()),
            outcome: Outcome::Refused,
            reason: Some("self-approval".into()),
            approval_id: Some("APR-1".into()),
            approver: None,
            target: Some(String::new()),
        });
        let bytes = encode_sealed_entry(&e);
        assert_eq!(bytes[0], 2);
        assert_eq!(decode_sealed_entry(&bytes).unwrap(), e);
        assert!(decode_sealed_entry(&bytes[..bytes.len() - 1]).is_err());
        // An outcome byte or a presence byte out of range fails closed.
        let tail = bytes.len() - 4; // the target's empty length prefix
        let mut bad_presence = bytes.clone();
        bad_presence[tail - 1] = 2;
        assert!(decode_sealed_entry(&bad_presence).is_err());
        let empty = encode_sealed_entry(&entry(AuditContext::default()));
        let mut bad_outcome = empty.clone();
        bad_outcome[empty.len() - 7] = 9;
        assert!(decode_sealed_entry(&bad_outcome).is_err());
    }
}
