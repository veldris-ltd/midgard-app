//! Deterministic, injective canonical encoding of an [`AuditRecord`] (no serde).
//!
//! Each field is written as `u32-LE length ‖ bytes`, prefixed by a schema version
//! and a domain-separation tag. Length-prefixing makes the encoding injective, so
//! two records with different field boundaries cannot hash to the same preimage
//! (e.g. actor="ab",action="c" ≠ actor="a",action="bc").

use crate::record::AuditRecord;

const SCHEMA_VERSION: u8 = 1;
const DOMAIN: &[u8] = b"midgard-audit-record-v1";

fn write_field(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

/// Encode a record to its canonical byte form.
pub fn canonical_record(r: &AuditRecord) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(SCHEMA_VERSION);
    write_field(&mut out, DOMAIN);
    write_field(&mut out, r.id.0.as_bytes());
    write_field(&mut out, r.actor.0.as_bytes());
    write_field(&mut out, r.action.0.as_bytes());
    write_field(&mut out, r.resource.0.as_bytes());
    write_field(&mut out, r.ts.0.as_bytes());
    out
}
