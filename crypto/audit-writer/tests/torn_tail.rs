//! VLD-ANB-MIDGARD-0040 — a crash part-way through an append no longer stops the
//! ledger opening, and the repair cannot be used to drop a complete entry. A partial
//! final frame after the checkpointed entries is cut off, reported and recorded; any
//! other partial frame fails closed, leaving the file as it was.
//!
//! The rest of the finding, a shortened ledger opening as a valid shorter chain, was
//! closed by the signed checkpoint and its mirror (MG-SEC-05): see
//! `truncation.rs::cutting_entries_off_the_end_is_detected` and
//! `checkpoint_mirror.rs::rolling_back_the_ledger_and_its_checkpoint_together_is_caught`.
//! [VLD-ANB-MIDGARD-0040]
#![cfg(any(feature = "test-backend", feature = "backend-fips"))]

mod common;

use std::path::{Path, PathBuf};

use audit_writer::{
    checkpoint_path, verify_ledger, AuditError, AuditWriter, FileLedger, LedgerId, LedgerStore,
    PinnedKey, Recovered, Sealer, LEDGER_REPAIRED_TORN_TAIL,
};
use common::provider;
use pqc_agility::SigId;

const ALG: SigId = SigId::MlDsa65;
const LEDGER: &str = "gov";

/// A stand-in for the KMS sealer that binds the AAD, as in `truncation.rs`.
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
    let d = std::env::temp_dir().join(format!("audit-torn-{tag}-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn id() -> LedgerId {
    LedgerId(LEDGER.into())
}

fn writer(store: FileLedger) -> AuditWriter<FileLedger> {
    let (scheme, sk, vk) = common::signer(ALG, 1);
    AuditWriter::new(store, scheme, sk, vk, id())
}

fn pin() -> PinnedKey {
    PinnedKey::new(common::signer(ALG, 1).2)
}

fn append(w: &mut AuditWriter<FileLedger>, n: usize) {
    for i in 0..n {
        w.append(common::record(&format!("r{i}"), "alice", "read", "obj:1"))
            .unwrap();
    }
}

/// A ledger made the way an installer makes one, with `entries` appended.
fn new_file_ledger(path: &Path, entries: usize) {
    let mut w = writer(FileLedger::create(path, Box::new(AadSealer), id()).unwrap());
    w.seal_checkpoint().unwrap();
    append(&mut w, entries);
}

fn open(path: &Path) -> Result<FileLedger, AuditError> {
    FileLedger::open(path, Box::new(AadSealer), id())
}

/// What a crash part-way through an append leaves: the 4-byte header of a frame
/// declaring 100 bytes, and the first 10 of them.
fn tear(path: &Path) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    f.write_all(&100u32.to_le_bytes()).unwrap();
    f.write_all(&[0xA5; 10]).unwrap();
}

/// The byte offset at which frame `n` (0-based) starts.
fn frame_start(path: &Path, n: usize) -> usize {
    let buf = std::fs::read(path).unwrap();
    let mut pos = 0;
    for _ in 0..n {
        let len = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
        pos += 4 + len as usize;
    }
    pos
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap().len()
}

// A partial frame after the checkpointed entries is what a crash during an append
// leaves: it is cut off, reported, recorded in the ledger once, and the ledger
// verifies and takes entries again. [VLD-ANB-MIDGARD-0040] [AC-28.2]
#[test]
fn a_torn_final_frame_is_truncated_and_reported() {
    let dir = scratch("torn");
    let path = dir.join("gov.ledger");
    new_file_ledger(&path, 3);
    let intact = file_len(&path);
    tear(&path);

    let ledger = open(&path).expect("a torn final frame does not stop the ledger opening");
    assert_eq!(
        ledger.recovered(),
        Some(&Recovered::TornTailTruncated { bytes: 14 })
    );
    assert_eq!(ledger.entries().len(), 3);
    assert_eq!(file_len(&path), intact, "cut back to the end of frame 3");

    // The repair is recorded in the ledger once, and the ledger takes entries again.
    let mut w = writer(ledger);
    assert_eq!(w.record_recovery("1000").unwrap(), Some(3));
    assert_eq!(w.record_recovery("1001").unwrap(), None, "recorded once");
    append(&mut w, 1);
    drop(w);

    let ledger = open(&path).unwrap();
    assert_eq!(ledger.recovered(), None);
    assert_eq!(ledger.entries().len(), 5);
    assert_eq!(ledger.checkpoint().unwrap().count, 5);
    let repair = &ledger.entries()[3].record;
    assert_eq!(repair.actor.0, "system");
    assert_eq!(repair.action.0, LEDGER_REPAIRED_TORN_TAIL);
    assert_eq!(repair.resource.0, "ledger:gov, 14 bytes cut");
    verify_ledger(
        ledger.entries(),
        ledger.checkpoint(),
        &provider(),
        &id(),
        &pin(),
    )
    .unwrap();
}

// A partial frame the checkpoint vouches for is a cut-off entry, not a crash: it
// fails closed and the file is left alone. [VLD-ANB-MIDGARD-0040] [AC-28.2]
#[test]
fn a_torn_frame_before_the_head_count_fails_closed() {
    let dir = scratch("before");
    let path = dir.join("gov.ledger");
    new_file_ledger(&path, 4);
    // Keep frame 4's header and 10 bytes of its body.
    let cut = frame_start(&path, 3) + 14;
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..cut]).unwrap();

    let err = open(&path)
        .err()
        .expect("a torn checkpointed frame fails closed");
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
    assert_eq!(file_len(&path), cut as u64, "nothing was cut off");
}

// One entry ahead of the checkpoint is the crash window between an entry and its
// checkpoint; a torn frame after that entry cannot come from a crash, and cutting it
// would let a ledger that fails as stale open by dropping its last entry.
// [VLD-ANB-MIDGARD-0040]
#[test]
fn a_torn_frame_after_an_entry_the_checkpoint_does_not_cover_fails_closed() {
    let dir = scratch("ahead");
    let path = dir.join("gov.ledger");
    new_file_ledger(&path, 3);
    let checkpoint = std::fs::read(checkpoint_path(&path)).unwrap();
    let mut w = writer(open(&path).unwrap());
    append(&mut w, 1);
    drop(w);
    std::fs::write(checkpoint_path(&path), checkpoint).unwrap();
    tear(&path);
    let torn = file_len(&path);

    let err = open(&path).err().expect("fails closed");
    assert!(
        matches!(
            err,
            AuditError::TornFrame {
                checkpoint: 3,
                found: 4
            }
        ),
        "{err:?}"
    );
    assert_eq!(file_len(&path), torn, "nothing was cut off");
}

// A ledger that has never been checkpointed and whose first append was torn is a
// creation that stopped part-way: it is cut back to empty and reported.
// [VLD-ANB-MIDGARD-0040]
#[test]
fn a_torn_first_frame_of_a_new_ledger_is_cut_off() {
    let dir = scratch("first");
    let path = dir.join("svc.ledger");
    drop(FileLedger::create(&path, Box::new(AadSealer), id()).unwrap());
    tear(&path);

    let ledger = FileLedger::open_or_create(&path, Box::new(AadSealer), id()).unwrap();
    assert_eq!(
        ledger.recovered(),
        Some(&Recovered::TornTailTruncated { bytes: 14 })
    );
    assert!(ledger.entries().is_empty());
    assert_eq!(file_len(&path), 0);
}

// With no checkpoint to say which frame a crash could have torn, a ledger with
// entries and a partial frame is not repaired: adoption still refuses it.
// [VLD-ANB-MIDGARD-0040]
#[test]
fn without_a_checkpoint_a_torn_frame_still_fails_closed() {
    let dir = scratch("uncheckpointed");
    let path = dir.join("gov.ledger");
    new_file_ledger(&path, 2);
    std::fs::remove_file(checkpoint_path(&path)).unwrap();
    tear(&path);
    let torn = file_len(&path);

    assert!(FileLedger::open_uncheckpointed(&path, Box::new(AadSealer), id()).is_err());
    assert!(FileLedger::open_or_create(&path, Box::new(AadSealer), id()).is_err());
    assert_eq!(file_len(&path), torn, "nothing was cut off");
}
