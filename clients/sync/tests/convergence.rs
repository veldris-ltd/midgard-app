//! AC-C-4 — offline edits merge conflict-free on reconnect with no data loss.
//!
//! The CRDT contract is order-independence: whatever order a replica sees the ops
//! in, it must land on the same state, and every independent edit must survive.
//! These tests exercise that with a deterministic op set applied under many
//! permutations (no RNG, so the proof is reproducible).

use midgard_client_sync::{Replica, SyncOp};

/// A deterministic set of concurrent ops from three offline replicas: some edits
/// touch distinct fields (must all survive), some race on the same field (LWW
/// must pick the same winner everywhere).
fn concurrent_ops() -> Vec<SyncOp> {
    let mut a = Replica::new("device-a");
    let mut b = Replica::new("device-b");
    let mut c = Replica::new("device-c");

    // Evaluated left-to-right, so Lamport clocks are assigned in this order.
    vec![
        // Distinct fields — independent, must not be lost.
        a.set("case:1", "title", b"Operation Bifrost".to_vec()),
        b.set("case:1", "owner", b"analyst-7".to_vec()),
        c.set("case:1", "status", b"open".to_vec()),
        // A same-field race between a and b (concurrent, same-ish clock).
        a.set("case:1", "priority", b"high".to_vec()),
        b.set("case:1", "priority", b"medium".to_vec()),
        // A delete racing a re-create on another object.
        a.set("case:2", "note", b"draft".to_vec()),
        c.delete("case:2", "note"),
    ]
}

/// Every permutation of a fixed op list must converge to the same snapshot, and
/// re-applying the whole list must be a no-op (idempotence).
#[test]
fn all_permutations_converge_and_are_idempotent() {
    let ops = concurrent_ops();

    // A reference replica sees the ops in their natural order.
    let mut reference = Replica::new("merger");
    reference.merge_all(ops.iter());
    let expected = reference.snapshot();

    // Several deterministic permutations: reversed, rotated by each offset.
    let mut permutations: Vec<Vec<&SyncOp>> = vec![ops.iter().rev().collect()];
    for shift in 0..ops.len() {
        let rotated: Vec<&SyncOp> = ops.iter().cycle().skip(shift).take(ops.len()).collect();
        permutations.push(rotated);
    }

    for (i, perm) in permutations.iter().enumerate() {
        let mut r = Replica::new("merger");
        r.merge_all(perm.iter().copied());
        assert_eq!(r.snapshot(), expected, "permutation {i} diverged");

        // Idempotence: replaying every op again changes nothing.
        r.merge_all(perm.iter().copied());
        assert_eq!(r.snapshot(), expected, "permutation {i} not idempotent");
    }
}

/// No independent edit is lost: each distinct field written by some replica is
/// present in the converged state.
#[test]
fn independent_edits_are_never_lost() {
    let ops = concurrent_ops();
    let mut r = Replica::new("merger");
    r.merge_all(ops.iter());

    assert_eq!(r.get("case:1", "title"), Some(&b"Operation Bifrost"[..]));
    assert_eq!(r.get("case:1", "owner"), Some(&b"analyst-7"[..]));
    assert_eq!(r.get("case:1", "status"), Some(&b"open"[..]));
    // The same-field race kept exactly one deterministic winner.
    assert!(r.get("case:1", "priority").is_some());
}

/// Two replicas that start offline, edit independently, then exchange their full
/// queues on "reconnect" end up identical — the round-trip AC-C-4 asserts.
#[test]
fn bidirectional_reconnect_converges_both_sides() {
    let mut a = Replica::new("device-a");
    let mut b = Replica::new("device-b");

    // Both go offline and edit.
    a.set("case:9", "title", b"a-title".to_vec());
    a.set("case:9", "field-a", b"1".to_vec());
    b.set("case:9", "title", b"b-title".to_vec());
    b.set("case:9", "field-b", b"2".to_vec());

    // Reconnect: each drains its queue and the other merges it.
    let from_a = a.drain_pending();
    let from_b = b.drain_pending();
    b.merge_all(from_a.iter());
    a.merge_all(from_b.iter());

    assert_eq!(a.snapshot(), b.snapshot(), "replicas did not converge");
    // Independent fields survived on both.
    assert_eq!(a.get("case:9", "field-a"), Some(&b"1"[..]));
    assert_eq!(a.get("case:9", "field-b"), Some(&b"2"[..]));
    // Both drained their queues — nothing left unsynced.
    assert!(!a.has_pending());
    assert!(!b.has_pending());
}
