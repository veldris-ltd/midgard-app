//! Canonical encoding must be injective: differing field boundaries must not
//! collide (no backend needed).

use audit_writer::{canonical_record, Action, Actor, AuditRecord, RecordId, Resource, Timestamp};

fn rec(actor: &str, action: &str) -> AuditRecord {
    AuditRecord {
        id: RecordId("id".into()),
        actor: Actor(actor.into()),
        action: Action(action.into()),
        resource: Resource("obj:1".into()),
        ts: Timestamp("2026-07-08T00:00:00Z".into()),
    }
}

#[test]
fn field_boundaries_are_injective() {
    // Naive concatenation would make these collide ("ab"+"c" == "a"+"bc").
    let a = canonical_record(&rec("ab", "c"));
    let b = canonical_record(&rec("a", "bc"));
    assert_ne!(a, b);
}

#[test]
fn identical_records_encode_identically() {
    assert_eq!(
        canonical_record(&rec("alice", "read")),
        canonical_record(&rec("alice", "read"))
    );
}
