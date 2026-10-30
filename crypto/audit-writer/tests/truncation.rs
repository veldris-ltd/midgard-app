//! MG-SEC-05 — the ledger cannot be quietly shortened, deleted, or checked with a
//! key of the attacker's choosing. Cutting entries off the end, deleting the ledger
//! or its checkpoint, re-signing it under another key, and forging a checkpoint for
//! a shortened chain all fail closed. [MG-SEC-05]
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};

use audit_writer::{
    checkpoint_path, verify_ledger, AuditError, AuditWriter, FileLedger, InMemoryLedger,
    LedgerCheckpoint, LedgerId, LedgerStore, PinnedKey, Sealer,
};
use pqc_agility::backend::test_backend::{provider, TestSig};
use pqc_agility::{SigId, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;
const LEDGER: &str = "gov";

/// A stand-in for the KMS sealer: binds the AAD, so position/ledger splices still
/// fail, without a KEM backend. Tamper-evidence of the bytes is not what these
/// tests are about.
struct AadSealer;
impl Sealer for AadSealer {
    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
        let mut o = (aad.len() as u32).to_le_bytes().to_vec();
        o.extend_from_slice(aad);
        o.extend_from_slice(plaintext);
        Ok(o)
    }
    fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
        let n = sealed
            .get(..4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| AuditError::Store("short".into()))?;
        if sealed.get(4..4 + n) != Some(aad) {
            return Err(AuditError::Store("aad mismatch".into()));
        }
        Ok(sealed[4 + n..].to_vec())
    }
}

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let d = std::env::temp_dir().join(format!("audit-trunc-{tag}-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn id() -> LedgerId {
    LedgerId(LEDGER.into())
}

fn writer<S: LedgerStore>(store: S, seed: u64) -> AuditWriter<S> {
    let (scheme, sk, vk) = common::signer(ALG, seed);
    AuditWriter::new(store, scheme, sk, vk, id())
}

fn pin(seed: u64) -> PinnedKey {
    PinnedKey::new(common::signer(ALG, seed).2)
}

fn append(w: &mut AuditWriter<impl LedgerStore>, n: usize) {
    for i in 0..n {
        w.append(common::record(&format!("r{i}"), "alice", "read", "obj:1"))
            .unwrap();
    }
}

/// A fresh ledger the way an installer makes one: create, then vouch for it empty.
fn new_file_ledger(path: &Path, entries: usize) {
    let mut w = writer(
        FileLedger::create(path, Box::new(AadSealer), id()).unwrap(),
        1,
    );
    w.seal_checkpoint().unwrap();
    append(&mut w, entries);
}

fn open(path: &Path) -> Result<FileLedger, AuditError> {
    FileLedger::open(path, Box::new(AadSealer), id())
}

/// Drop the last `k` length-framed entries from the ledger file.
fn cut_tail(path: &Path, k: usize) {
    let buf = std::fs::read(path).unwrap();
    let mut frames = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        frames.push(pos);
        let len = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
        pos += 4 + len as usize;
    }
    let keep = frames.len() - k;
    let end = frames.get(keep).copied().unwrap_or(buf.len());
    std::fs::write(path, &buf[..end]).unwrap();
}

#[test]
fn an_intact_ledger_verifies_under_the_pin_across_a_reopen() {
    let dir = scratch("intact");
    let path = dir.join("gov.ledger");
    new_file_ledger(&path, 3);
    let ledger = open(&path).unwrap();
    verify_ledger(
        ledger.entries(),
        ledger.checkpoint(),
        &provider(),
        &id(),
        &pin(1),
    )
    .unwrap();
    assert_eq!(ledger.checkpoint().unwrap().count, 3);

    // Appending after a reopen keeps chain and checkpoint in step.
    let mut w = writer(ledger, 1);
    append(&mut w, 2);
    let ledger = open(&path).unwrap();
    verify_ledger(
        ledger.entries(),
        ledger.checkpoint(),
        &provider(),
        &id(),
        &pin(1),
    )
    .unwrap();
    assert_eq!(ledger.entries().len(), 5);
}

#[test]
fn cutting_entries_off_the_end_is_detected() {
    let dir = scratch("tail");
    let path = dir.join("gov.ledger");
    new_file_ledger(&path, 4);

    cut_tail(&path, 1);
    let err = open(&path).err().expect("a shortened ledger must not open");
    assert!(
        matches!(
            err,
            AuditError::Truncated {
                checkpoint: 4,
                found: 3
            }
        ),
        "{err:?}"
    );

    // Emptying the file entirely is the same attack.
    cut_tail(&path, 3);
    let err = open(&path).err().expect("an emptied ledger must not open");
    assert!(
        matches!(
            err,
            AuditError::Truncated {
                checkpoint: 4,
                found: 0
            }
        ),
        "{err:?}"
    );
    // ...including through the create-on-first-start path.
    let err = FileLedger::open_or_create(&path, Box::new(AadSealer), id())
        .err()
        .expect("open_or_create must not re-create over a checkpoint");
    assert!(matches!(err, AuditError::Truncated { .. }), "{err:?}");
}

#[test]
fn truncation_is_detected_in_memory_too() {
    let mut w = writer(InMemoryLedger::new(), 1);
    append(&mut w, 3);
    let cp = w.checkpoint().cloned();
    let entries = w.entries();
    for keep in [2, 0] {
        let err =
            verify_ledger(&entries[..keep], cp.as_ref(), &provider(), &id(), &pin(1)).unwrap_err();
        assert!(matches!(err, AuditError::Truncated { .. }), "{err:?}");
    }
    verify_ledger(entries, cp.as_ref(), &provider(), &id(), &pin(1)).unwrap();
}

#[test]
fn a_deleted_ledger_is_an_error_not_a_fresh_start() {
    let dir = scratch("deleted");
    let path = dir.join("gov.ledger");
    new_file_ledger(&path, 2);

    std::fs::remove_file(&path).unwrap();
    let err = open(&path).err().expect("a deleted ledger must not open");
    assert!(matches!(err, AuditError::LedgerMissing(_)), "{err:?}");
    // The checkpoint left behind also stops a quiet re-create.
    let err = FileLedger::open_or_create(&path, Box::new(AadSealer), id())
        .err()
        .expect("a ledger deleted under its checkpoint must not be re-created");
    assert!(matches!(err, AuditError::LedgerMissing(_)), "{err:?}");
    // And `create` never overwrites.
    assert!(matches!(
        FileLedger::create(&path, Box::new(AadSealer), id()).err(),
        Some(AuditError::LedgerExists(_))
    ));

    // Deleting both is still refused by `open`, which is what a ledger an installer
    // created is opened with.
    std::fs::remove_file(checkpoint_path(&path)).unwrap();
    let err = open(&path).err().expect("no ledger, no open");
    assert!(matches!(err, AuditError::LedgerMissing(_)), "{err:?}");
}

#[test]
fn a_deleted_checkpoint_is_an_error() {
    let dir = scratch("nocp");
    let path = dir.join("gov.ledger");
    new_file_ledger(&path, 2);
    std::fs::remove_file(checkpoint_path(&path)).unwrap();
    let err = open(&path).err().expect("no checkpoint, no open");
    assert!(matches!(err, AuditError::CheckpointMissing), "{err:?}");
    let err = FileLedger::open_or_create(&path, Box::new(AadSealer), id())
        .err()
        .expect("entries without a checkpoint are not a first start");
    assert!(matches!(err, AuditError::CheckpointMissing), "{err:?}");

    // verify_ledger refuses a missing checkpoint on its own, too.
    let mut w = writer(InMemoryLedger::new(), 1);
    append(&mut w, 1);
    let err = verify_ledger(w.entries(), None, &provider(), &id(), &pin(1)).unwrap_err();
    assert!(matches!(err, AuditError::CheckpointMissing), "{err:?}");
}

#[test]
fn a_forged_checkpoint_for_a_shortened_ledger_is_refused() {
    let dir = scratch("forged");
    let path = dir.join("gov.ledger");
    new_file_ledger(&path, 4);

    // The attacker cuts the tail and writes a checkpoint that matches the shorter
    // chain, signed with a key of their own: it passes the keyless open...
    cut_tail(&path, 2);
    // The two entries that remain (entry hashes are deterministic).
    let mut remaining = writer(InMemoryLedger::new(), 1);
    append(&mut remaining, 2);
    let (scheme, attacker_sk, _) = common::signer(ALG, 666);
    let forged =
        LedgerCheckpoint::sign(scheme.as_ref(), &attacker_sk, &id(), remaining.entries()).unwrap();
    std::fs::write(checkpoint_path(&path), forged.encode()).unwrap();
    let ledger = open(&path).expect("structurally consistent");
    assert_eq!(ledger.entries().len(), 2);

    // ...but not the pinned verification.
    let err = verify_ledger(
        ledger.entries(),
        ledger.checkpoint(),
        &provider(),
        &id(),
        &pin(1),
    )
    .unwrap_err();
    assert!(matches!(err, AuditError::CheckpointInvalid(_)), "{err:?}");
}

#[test]
fn a_ledger_checked_with_the_wrong_key_is_refused() {
    // A ledger (and its checkpoint) signed end to end by a swapped-in keystore
    // verifies under that keystore's own key — which is why the key it is checked
    // with is pinned from elsewhere.
    let mut forged = writer(InMemoryLedger::new(), 666);
    append(&mut forged, 3);
    let attacker_vk: VerifyingKey = common::signer(ALG, 666).2;
    verify_ledger(
        forged.entries(),
        forged.checkpoint(),
        &provider(),
        &id(),
        &PinnedKey::new(attacker_vk.clone()),
    )
    .expect("self-consistent under its own key");

    let pinned = pin(1);
    // The signer is not the pinned key...
    assert!(matches!(
        pinned.check(&attacker_vk),
        Err(AuditError::KeyNotPinned)
    ));
    // ...and nothing it signed verifies under the pin.
    let err = verify_ledger(
        forged.entries(),
        forged.checkpoint(),
        &provider(),
        &id(),
        &pinned,
    )
    .unwrap_err();
    assert!(
        matches!(err, AuditError::SignatureInvalid { seq: 0 }),
        "{err:?}"
    );
    // An empty forged ledger is caught by its checkpoint signature alone.
    let mut empty = writer(InMemoryLedger::new(), 666);
    empty.seal_checkpoint().unwrap();
    let err = verify_ledger(&[], empty.checkpoint(), &provider(), &id(), &pinned).unwrap_err();
    assert!(matches!(err, AuditError::CheckpointInvalid(_)), "{err:?}");
}

#[test]
fn a_crash_before_the_checkpoint_is_tolerated_but_only_by_one_entry() {
    let mut w = writer(InMemoryLedger::new(), 1);
    append(&mut w, 2);
    let cp = w.checkpoint().cloned();
    append(&mut w, 2);
    let entries = w.entries();
    // One entry ahead: the window between an entry's fsync and its checkpoint.
    verify_ledger(&entries[..3], cp.as_ref(), &provider(), &id(), &pin(1)).unwrap();
    // Two ahead: a stale (rolled-back) checkpoint.
    let err = verify_ledger(entries, cp.as_ref(), &provider(), &id(), &pin(1)).unwrap_err();
    assert!(
        matches!(
            err,
            AuditError::CheckpointStale {
                checkpoint: 2,
                found: 4
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_rewritten_tail_does_not_match_the_checkpoint() {
    // Even the genuine signer, replacing the last entry with a different one, leaves
    // a head the checkpoint does not name.
    let mut original = writer(InMemoryLedger::new(), 1);
    append(&mut original, 3);
    let cp = original.checkpoint().cloned();

    let mut rewritten = writer(InMemoryLedger::new(), 1);
    append(&mut rewritten, 2);
    rewritten
        .append(common::record("r2", "alice", "delete", "obj:1"))
        .unwrap();
    let err = verify_ledger(
        rewritten.entries(),
        cp.as_ref(),
        &provider(),
        &id(),
        &pin(1),
    )
    .unwrap_err();
    assert!(
        matches!(err, AuditError::CheckpointMismatch { seq: 2 }),
        "{err:?}"
    );
}

#[test]
fn a_checkpoint_for_another_ledger_is_refused() {
    let mut w = writer(InMemoryLedger::new(), 1);
    append(&mut w, 1);
    let err = verify_ledger(
        w.entries(),
        w.checkpoint(),
        &provider(),
        &LedgerId("other".into()),
        &pin(1),
    )
    .unwrap_err();
    // The chain's genesis is bound to the ledger id, so the chain breaks first.
    assert!(
        matches!(err, AuditError::ChainBroken { seq: 0, .. }),
        "{err:?}"
    );

    let scheme = TestSig::new(ALG);
    let sk = common::signing_key(ALG, 1);
    let other = LedgerCheckpoint::sign(&scheme, &sk, &LedgerId("other".into()), &[]).unwrap();
    let err = verify_ledger(&[], Some(&other), &provider(), &id(), &pin(1)).unwrap_err();
    assert!(matches!(err, AuditError::CheckpointInvalid(_)), "{err:?}");
}
