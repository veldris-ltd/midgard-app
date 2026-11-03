//! Deterministic, injective canonical encoding of an [`AuditRecord`] (no serde).
//!
//! Each field is written as `u32-LE length ‖ bytes`, prefixed by a schema version
//! and a domain-separation tag. Length-prefixing makes the encoding injective, so
//! two records with different field boundaries cannot hash to the same preimage
//! (e.g. actor="ab",action="c" ≠ actor="a",action="bc").
//!
//! A record with an empty [`AuditContext`](crate::record::AuditContext) is encoded
//! in the version 1 layout byte for byte, so every entry written before the context
//! existed re-hashes to its stored entry hash. A record with a context is encoded in
//! the version 2 layout under its own domain tag: the five fields, the outcome byte,
//! then each optional field as a presence byte and, when present, its length-prefixed
//! bytes. [VLD-ANB-MIDGARD-0052]

use crate::record::AuditRecord;

const SCHEMA_VERSION: u8 = 1;
const DOMAIN: &[u8] = b"midgard-audit-record-v1";
const SCHEMA_VERSION_CONTEXT: u8 = 2;
const DOMAIN_CONTEXT: &[u8] = b"midgard-audit-record-v2";

fn write_field(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

/// Encode a record to its canonical byte form.
pub fn canonical_record(r: &AuditRecord) -> Vec<u8> {
    let empty = r.context.is_empty();
    let mut out = Vec::new();
    if empty {
        out.push(SCHEMA_VERSION);
        write_field(&mut out, DOMAIN);
    } else {
        out.push(SCHEMA_VERSION_CONTEXT);
        write_field(&mut out, DOMAIN_CONTEXT);
    }
    write_field(&mut out, r.id.0.as_bytes());
    write_field(&mut out, r.actor.0.as_bytes());
    write_field(&mut out, r.action.0.as_bytes());
    write_field(&mut out, r.resource.0.as_bytes());
    write_field(&mut out, r.ts.0.as_bytes());
    if !empty {
        out.push(r.context.outcome.as_u8());
        for field in r.context.optional_fields() {
            match field {
                Some(value) => {
                    out.push(1);
                    write_field(&mut out, value.as_bytes());
                }
                None => out.push(0),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Action, Actor, AuditContext, Outcome, RecordId, Resource, Timestamp};

    fn record(context: AuditContext) -> AuditRecord {
        AuditRecord {
            id: RecordId("r1".into()),
            actor: Actor("al".into()),
            action: Action("PERMIT Read".into()),
            resource: Resource("o/1".into()),
            ts: Timestamp("17".into()),
            context,
        }
    }

    /// Ledgers written before the context existed must still verify, so an empty
    /// context is the version 1 preimage exactly as it was. [VLD-ANB-MIDGARD-0052]
    /// [AC-26.1]
    #[test]
    fn v1_canonical_bytes_are_unchanged_for_an_empty_context() {
        // Pinned: the bytes the original encoder produced for this record.
        let expected: &[u8] = &[
            1, 23, 0, 0, 0, b'm', b'i', b'd', b'g', b'a', b'r', b'd', b'-', b'a', b'u', b'd', b'i',
            b't', b'-', b'r', b'e', b'c', b'o', b'r', b'd', b'-', b'v', b'1', 2, 0, 0, 0, b'r',
            b'1', 2, 0, 0, 0, b'a', b'l', 11, 0, 0, 0, b'P', b'E', b'R', b'M', b'I', b'T', b' ',
            b'R', b'e', b'a', b'd', 3, 0, 0, 0, b'o', b'/', b'1', 2, 0, 0, 0, b'1', b'7',
        ];
        assert_eq!(canonical_record(&record(AuditContext::default())), expected);
    }

    /// Every context field is inside the hashed preimage: changing any one changes
    /// the bytes, and a stated context never collides with an empty one.
    /// [VLD-ANB-MIDGARD-0052] [AC-26.1]
    #[test]
    fn context_fields_are_part_of_the_hash() {
        let base = AuditContext {
            source: Some("127.0.0.1".into()),
            session: Some("alice:1".into()),
            outcome: Outcome::Refused,
            reason: Some("self-approval".into()),
            approval_id: Some("APR-1".into()),
            approver: Some("bob".into()),
            target: Some("aircraft/1".into()),
        };
        let encoded = canonical_record(&record(base.clone()));
        assert_ne!(encoded, canonical_record(&record(AuditContext::default())));
        assert_eq!(encoded[0], 2);

        let variants = [
            AuditContext {
                source: Some("127.0.0.2".into()),
                ..base.clone()
            },
            AuditContext {
                session: None,
                ..base.clone()
            },
            AuditContext {
                outcome: Outcome::Deny,
                ..base.clone()
            },
            AuditContext {
                reason: Some("self-approvaL".into()),
                ..base.clone()
            },
            AuditContext {
                approval_id: Some("APR-2".into()),
                ..base.clone()
            },
            AuditContext {
                approver: Some(String::new()),
                ..base.clone()
            },
            AuditContext {
                target: None,
                ..base.clone()
            },
        ];
        for v in variants {
            assert_ne!(canonical_record(&record(v.clone())), encoded, "{v:?}");
        }
        // A present empty string is not an absent field.
        let a = AuditContext {
            outcome: Outcome::Info,
            reason: Some(String::new()),
            ..Default::default()
        };
        let b = AuditContext {
            outcome: Outcome::Info,
            ..Default::default()
        };
        assert_ne!(canonical_record(&record(a)), canonical_record(&record(b)));
    }
}
