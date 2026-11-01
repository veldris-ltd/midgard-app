//! MG-SEC-05 — a checkpoint mirror off the server catches what the local checkpoint
//! cannot: the ledger and its checkpoint rolled back together, both deleted, or the
//! ledger replaced by another of the same length. A mirror behind the ledger is caught
//! up, a mirror that cannot be written stops the ledger growing, and a lost mirror is
//! re-seeded only by an explicit step. [MG-SEC-05]
#![cfg(any(feature = "test-backend", feature = "backend-fips"))]

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use audit_writer::{
    checkpoint_path, verify_ledger, AuditError, AuditWriter, CheckpointMirror,
    FileCheckpointMirror, FileLedger, LedgerCheckpoint, LedgerId, LedgerStore, PinnedKey, Sealer,
};
use common::provider;
use pqc_agility::SigId;

const ALG: SigId = SigId::MlDsa65;
const LEDGER: &str = "gov";

/// Binds the AAD without a KEM backend, as in `truncation.rs`.
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

/// A state directory and, apart from it, a mirror directory.
struct Dirs {
    root: PathBuf,
    ledger: PathBuf,
    mirror: PathBuf,
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn scratch(tag: &str) -> Dirs {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let root = std::env::temp_dir().join(format!(
        "audit-mirror-{tag}-{}-{}",
        std::process::id(),
        uniq
    ));
    let state = root.join("state");
    let mirror = root.join("mirror");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&mirror).unwrap();
    Dirs {
        ledger: state.join("gov.ledger"),
        root,
        mirror,
    }
}

fn id() -> LedgerId {
    LedgerId(LEDGER.into())
}

fn mirror(dir: &Path) -> Box<dyn CheckpointMirror> {
    Box::new(FileCheckpointMirror::new(dir).unwrap())
}

fn mirrored(dir: &Path) -> Option<LedgerCheckpoint> {
    FileCheckpointMirror::new(dir)
        .unwrap()
        .latest(&id())
        .unwrap()
}

fn writer<S: LedgerStore>(store: S, seed: u64) -> AuditWriter<S> {
    let (scheme, sk, vk) = common::signer(ALG, seed);
    AuditWriter::new(store, scheme, sk, vk, id())
}

fn append(w: &mut AuditWriter<impl LedgerStore>, tag: &str, n: usize) {
    for i in 0..n {
        w.append(common::record(
            &format!("{tag}{i}"),
            "alice",
            "read",
            "obj:1",
        ))
        .unwrap();
    }
}

/// A new mirrored ledger, vouched for empty, with `entries` appended.
fn new_mirrored(d: &Dirs, entries: usize) -> AuditWriter<FileLedger> {
    let ledger =
        FileLedger::create_mirrored(&d.ledger, Box::new(AadSealer), id(), mirror(&d.mirror))
            .unwrap();
    let mut w = writer(ledger, 1);
    w.seal_checkpoint().unwrap();
    append(&mut w, "r", entries);
    w
}

fn open_plain(path: &Path) -> Result<FileLedger, AuditError> {
    FileLedger::open(path, Box::new(AadSealer), id())
}

fn open_mirrored(d: &Dirs) -> Result<FileLedger, AuditError> {
    FileLedger::open_mirrored(&d.ledger, Box::new(AadSealer), id(), mirror(&d.mirror))
}

/// Copy the ledger and its checkpoint aside, as a backup would.
fn back_up(d: &Dirs) -> (Vec<u8>, Vec<u8>) {
    (
        std::fs::read(&d.ledger).unwrap(),
        std::fs::read(checkpoint_path(&d.ledger)).unwrap(),
    )
}

fn restore(d: &Dirs, (ledger, checkpoint): &(Vec<u8>, Vec<u8>)) {
    std::fs::write(&d.ledger, ledger).unwrap();
    std::fs::write(checkpoint_path(&d.ledger), checkpoint).unwrap();
}

#[test]
fn every_checkpoint_is_mirrored_signed_and_the_ledger_reopens_against_it() {
    let d = scratch("mirrored");
    drop(new_mirrored(&d, 3));
    let m = mirrored(&d.mirror).expect("the mirror holds the checkpoint");
    assert_eq!(m.count, 3);
    // The mirrored copy is the signed checkpoint itself, checkable under the pin.
    let pin = PinnedKey::new(common::signer(ALG, 1).2);
    m.verify_signature(&provider(), &id(), pin.key()).unwrap();

    let ledger = open_mirrored(&d).unwrap();
    assert!(ledger.is_mirrored());
    assert_eq!(ledger.checkpoint(), Some(&m));
    verify_ledger(
        ledger.entries(),
        ledger.checkpoint(),
        &provider(),
        &id(),
        &pin,
    )
    .unwrap();
    let mut w = writer(ledger, 1);
    append(&mut w, "s", 2);
    assert_eq!(mirrored(&d.mirror).unwrap().count, 5);
}

#[test]
fn rolling_back_the_ledger_and_its_checkpoint_together_is_caught() {
    let d = scratch("rollback");
    let mut w = new_mirrored(&d, 2);
    let backup = back_up(&d);
    append(&mut w, "s", 3);
    drop(w);

    // The older pair is self-consistent: on its own disk nothing is wrong...
    restore(&d, &backup);
    let rolled_back = open_plain(&d.ledger).expect("consistent with its own checkpoint");
    assert_eq!(rolled_back.entries().len(), 2);
    // ...but the mirror remembers five entries.
    let err = open_mirrored(&d)
        .err()
        .expect("a rolled-back ledger must not open");
    assert!(
        matches!(
            err,
            AuditError::RolledBack {
                mirrored: 5,
                local: 2
            }
        ),
        "{err:?}"
    );
    let err = FileLedger::open_or_create_mirrored(
        &d.ledger,
        Box::new(AadSealer),
        id(),
        mirror(&d.mirror),
    )
    .err()
    .expect("nor through the create-on-first-start path");
    assert!(matches!(err, AuditError::RolledBack { .. }), "{err:?}");
    // The refused open left the mirror where it was.
    assert_eq!(mirrored(&d.mirror).unwrap().count, 5);
}

#[test]
fn deleting_the_ledger_and_its_checkpoint_is_caught() {
    let d = scratch("deleted");
    drop(new_mirrored(&d, 2));
    std::fs::remove_file(&d.ledger).unwrap();
    std::fs::remove_file(checkpoint_path(&d.ledger)).unwrap();

    // Without the mirror, a store that creates its own ledger cannot tell this from a
    // first start; with it, it can.
    let err = FileLedger::open_or_create_mirrored(
        &d.ledger,
        Box::new(AadSealer),
        id(),
        mirror(&d.mirror),
    )
    .err()
    .expect("a mirrored ledger deleted with its checkpoint is not a first start");
    assert!(matches!(err, AuditError::LedgerMissing(_)), "{err:?}");
    assert!(!d.ledger.exists(), "the refused open does not re-create it");
    // Nor is a new one created in its place.
    let err = FileLedger::create_mirrored(&d.ledger, Box::new(AadSealer), id(), mirror(&d.mirror))
        .err()
        .expect("the mirror vouches that a ledger existed");
    assert!(matches!(err, AuditError::LedgerExists(_)), "{err:?}");
    assert!(!d.ledger.exists());
    let err = open_mirrored(&d).err().expect("no ledger, no open");
    assert!(matches!(err, AuditError::LedgerMissing(_)), "{err:?}");

    // A fresh start, with an empty mirror, is still a fresh start.
    let fresh = scratch("fresh");
    let ledger = FileLedger::open_or_create_mirrored(
        &fresh.ledger,
        Box::new(AadSealer),
        id(),
        mirror(&fresh.mirror),
    )
    .unwrap();
    let mut w = writer(ledger, 1);
    append(&mut w, "r", 1);
    assert_eq!(mirrored(&fresh.mirror).unwrap().count, 1);
}

#[test]
fn a_ledger_replaced_by_another_of_the_same_length_is_caught() {
    let d = scratch("replaced");
    drop(new_mirrored(&d, 3));

    // Another ledger, same id, same key, same length, different records, with a
    // checkpoint that matches it.
    let other = scratch("other");
    let mut w = writer(
        FileLedger::create(&other.ledger, Box::new(AadSealer), id()).unwrap(),
        1,
    );
    w.seal_checkpoint().unwrap();
    append(&mut w, "forged", 3);
    drop(w);
    std::fs::copy(&other.ledger, &d.ledger).unwrap();
    std::fs::copy(checkpoint_path(&other.ledger), checkpoint_path(&d.ledger)).unwrap();

    open_plain(&d.ledger).expect("consistent with its own checkpoint");
    let err = open_mirrored(&d).err().expect("not the mirrored head");
    assert!(
        matches!(err, AuditError::MirrorMismatch { seq: 2 }),
        "{err:?}"
    );
}

#[test]
fn a_local_checkpoint_behind_the_mirror_is_refused() {
    let d = scratch("ahead");
    let mut w = new_mirrored(&d, 2);
    let old_checkpoint = std::fs::read(checkpoint_path(&d.ledger)).unwrap();
    append(&mut w, "s", 1);
    drop(w);

    // Only the local checkpoint is rolled back: the ledger is one entry ahead of it,
    // which a crash can leave and `open` tolerates, but the mirror is ahead of it.
    std::fs::write(checkpoint_path(&d.ledger), old_checkpoint).unwrap();
    open_plain(&d.ledger).expect("one entry ahead is tolerated locally");
    let err = open_mirrored(&d).err().expect("the mirror is ahead");
    assert!(
        matches!(
            err,
            AuditError::RolledBack {
                mirrored: 3,
                local: 2
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_mirror_behind_the_ledger_is_caught_up_by_the_next_checkpoint() {
    let d = scratch("behind");
    drop(new_mirrored(&d, 2));
    // Entries written while the mirror was not configured (or a crash between the
    // local and the mirror write) leave the mirror behind.
    let mut w = writer(open_plain(&d.ledger).unwrap(), 1);
    append(&mut w, "s", 2);
    drop(w);
    assert_eq!(mirrored(&d.mirror).unwrap().count, 2);

    let ledger = open_mirrored(&d).expect("a mirror behind the ledger is not an error");
    // Nothing is published before the caller has verified the ledger...
    assert_eq!(mirrored(&d.mirror).unwrap().count, 2);
    // ...and the next checkpoint brings the mirror up to date.
    let mut w = writer(ledger, 1);
    append(&mut w, "t", 1);
    assert_eq!(mirrored(&d.mirror).unwrap().count, 5);
}

#[test]
fn a_missing_mirror_fails_closed_until_it_is_re_seeded() {
    let d = scratch("missing");
    drop(new_mirrored(&d, 2));
    let mirror_file = FileCheckpointMirror::new(&d.mirror)
        .unwrap()
        .path_for(&id());
    std::fs::remove_file(&mirror_file).unwrap();

    let err = open_mirrored(&d)
        .err()
        .expect("entries, and no mirrored checkpoint");
    assert!(matches!(err, AuditError::MirrorMissing(_)), "{err:?}");

    // The operator verifies the ledger under the pin, then re-seeds the mirror.
    let mut ledger = open_plain(&d.ledger).unwrap();
    verify_ledger(
        ledger.entries(),
        ledger.checkpoint(),
        &provider(),
        &id(),
        &PinnedKey::new(common::signer(ALG, 1).2),
    )
    .unwrap();
    ledger.start_mirroring(mirror(&d.mirror)).unwrap();
    assert_eq!(mirrored(&d.mirror).unwrap().count, 2);
    drop(ledger);
    open_mirrored(&d).expect("re-seeded");

    // Re-seeding never moves a mirror back: a rolled-back ledger is still refused.
    let backup = back_up(&d);
    let mut w = writer(open_mirrored(&d).unwrap(), 1);
    append(&mut w, "s", 2);
    drop(w);
    restore(&d, &backup);
    let mut rolled_back = open_plain(&d.ledger).unwrap();
    let err = rolled_back
        .start_mirroring(mirror(&d.mirror))
        .expect_err("the mirror is ahead");
    assert!(matches!(err, AuditError::RolledBack { .. }), "{err:?}");
    assert_eq!(mirrored(&d.mirror).unwrap().count, 4);

    // An empty ledger with no mirrored checkpoint opens: nothing can have been lost.
    let empty = scratch("empty");
    drop(new_mirrored(&empty, 0));
    std::fs::remove_file(
        FileCheckpointMirror::new(&empty.mirror)
            .unwrap()
            .path_for(&id()),
    )
    .unwrap();
    open_mirrored(&empty).expect("an empty ledger and an empty mirror");
}

#[test]
fn a_mirror_that_cannot_be_written_stops_the_ledger() {
    let d = scratch("unwritable");
    let mut w = new_mirrored(&d, 1);

    // The mirror volume goes away.
    std::fs::remove_dir_all(&d.mirror).unwrap();
    let err = w
        .append(common::record("r1", "alice", "read", "obj:1"))
        .expect_err("an append whose checkpoint is not mirrored fails");
    assert!(matches!(err, AuditError::Mirror(_)), "{err:?}");
    // The entry is durable and checkpointed locally, one ahead of the mirror...
    assert_eq!(w.entries().len(), 2);
    // ...and the ledger grows no further while the mirror cannot take a checkpoint.
    let err = w
        .append(common::record("r2", "alice", "read", "obj:1"))
        .expect_err("refused while the mirror lags");
    assert!(matches!(err, AuditError::Mirror(_)), "{err:?}");
    assert_eq!(w.entries().len(), 2);

    // Back, the mirror catches up and the ledger moves on.
    std::fs::create_dir_all(&d.mirror).unwrap();
    w.append(common::record("r2", "alice", "read", "obj:1"))
        .unwrap();
    assert_eq!(mirrored(&d.mirror).unwrap().count, 3);
    drop(w);
    open_mirrored(&d).unwrap();

    // A mirror directory that is not there is refused up front.
    assert!(matches!(
        FileCheckpointMirror::new(d.root.join("not-mounted")),
        Err(AuditError::Mirror(_))
    ));
}

#[test]
fn a_file_mirror_never_moves_back() {
    let d = scratch("forward");
    let mut w = new_mirrored(&d, 1);
    let one = w.checkpoint().cloned().unwrap();
    append(&mut w, "s", 1);
    let m = FileCheckpointMirror::new(&d.mirror).unwrap();
    assert!(matches!(m.publish(&one), Err(AuditError::Mirror(_))));
    // The same checkpoint again is a no-op, not an error.
    m.publish(w.checkpoint().unwrap()).unwrap();
    // Another head at the same count is refused.
    let mut rewritten = writer(audit_writer::InMemoryLedger::new(), 1);
    append(&mut rewritten, "x", 2);
    assert!(matches!(
        m.publish(rewritten.checkpoint().unwrap()),
        Err(AuditError::Mirror(_))
    ));
    assert_eq!(mirrored(&d.mirror).unwrap().count, 2);
}

/// A deployment's own sink (anything that implements [`CheckpointMirror`]), here one
/// held in memory and shared, so the test can look into it.
#[derive(Clone, Default)]
struct RemoteSink(Arc<Mutex<Option<LedgerCheckpoint>>>);
impl CheckpointMirror for RemoteSink {
    fn latest(&self, _: &LedgerId) -> Result<Option<LedgerCheckpoint>, AuditError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn publish(&self, checkpoint: &LedgerCheckpoint) -> Result<(), AuditError> {
        let mut held = self.0.lock().unwrap();
        if let Some(prev) = held.as_ref() {
            audit_writer::mirror::check_forward(prev, checkpoint)?;
        }
        *held = Some(checkpoint.clone());
        Ok(())
    }
    fn describe(&self, _: &LedgerId) -> String {
        "remote sink".into()
    }
}

#[test]
fn any_sink_can_be_the_mirror() {
    let d = scratch("sink");
    let sink = RemoteSink::default();
    let ledger =
        FileLedger::create_mirrored(&d.ledger, Box::new(AadSealer), id(), Box::new(sink.clone()))
            .unwrap();
    let mut w = writer(ledger, 1);
    w.seal_checkpoint().unwrap();
    let backup = back_up(&d);
    append(&mut w, "r", 3);
    drop(w);
    assert_eq!(sink.latest(&id()).unwrap().unwrap().count, 3);

    restore(&d, &backup);
    let err = FileLedger::open_mirrored(&d.ledger, Box::new(AadSealer), id(), Box::new(sink))
        .err()
        .expect("the sink remembers three entries");
    assert!(
        matches!(
            err,
            AuditError::RolledBack {
                mirrored: 3,
                local: 0
            }
        ),
        "{err:?}"
    );
}
