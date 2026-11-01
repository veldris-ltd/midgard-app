//! The checkpoint mirror: a copy of the ledger's signed checkpoint kept **off the
//! server**, so that rolling the ledger and its checkpoint back together is caught.
//! [MG-SEC-05]
//!
//! A checkpoint beside the ledger catches a ledger cut short of it, but whoever can
//! write the state directory can roll *both* back to an earlier consistent pair (or
//! delete both and restore an old backup), and the pair still agrees. A
//! [`CheckpointMirror`] is a second place every checkpoint is also published to — a
//! directory on another mount ([`FileCheckpointMirror`]), or any remote sink a
//! deployment implements the trait for. The mirror only ever moves forward: its entry
//! count never goes down, and at the same count its head never changes.
//!
//! On open, a ledger with a mirror must not be behind it:
//!
//! - the local checkpoint may not vouch for fewer entries than the mirror's
//!   ([`AuditError::RolledBack`]), and the ledger must hold the mirrored head at the
//!   mirrored position ([`AuditError::MirrorMismatch`]);
//! - a mirror that holds nothing for a ledger that holds entries fails closed
//!   ([`AuditError::MirrorMissing`]): it is re-seeded only by an explicit operator
//!   step ([`crate::FileLedger::start_mirroring`]) after the ledger was verified under
//!   the pinned key;
//! - a mirror *behind* the ledger is not an error — a crash between the local and the
//!   mirror write, or a write while the mirror was not configured, leaves exactly
//!   that — and the next checkpoint brings it up to date.
//!
//! Every checkpoint is written locally first and then to the mirror, so the mirror is
//! never legitimately ahead of the local one. A mirror write that fails fails the
//! append that caused it, and the ledger then refuses further appends until the mirror
//! takes the checkpoint: an audit ledger stops rather than grow unmirrored.

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::chain::genesis_hash;
use crate::checkpoint::LedgerCheckpoint;
use crate::error::AuditError;
use crate::record::{LedgerId, SealedEntry};

/// Somewhere off the server that keeps the latest signed checkpoint of each ledger,
/// and never lets it move back. [MG-SEC-05]
///
/// `Send + Sync` because a ledger holding a mirror sits behind a service's shared
/// state, like its [`Sealer`](crate::Sealer).
pub trait CheckpointMirror: Send + Sync {
    /// The latest checkpoint published for `ledger_id`, or `None` if the mirror holds
    /// none. An unreachable mirror is an error, never `None`.
    fn latest(&self, ledger_id: &LedgerId) -> Result<Option<LedgerCheckpoint>, AuditError>;
    /// Publish `checkpoint` durably. Must refuse one that would move the mirror back:
    /// a lower entry count, or the same count with another head.
    fn publish(&self, checkpoint: &LedgerCheckpoint) -> Result<(), AuditError>;
    /// Where the mirror keeps `ledger_id`'s checkpoint, for error messages.
    fn describe(&self, ledger_id: &LedgerId) -> String;
}

/// Refuse `next` if publishing it over `prev` would move a mirror back. [MG-SEC-05]
pub fn check_forward(prev: &LedgerCheckpoint, next: &LedgerCheckpoint) -> Result<(), AuditError> {
    if prev.ledger_id != next.ledger_id {
        return Err(AuditError::Mirror(format!(
            "it holds ledger {:?}, not {:?}",
            prev.ledger_id.0, next.ledger_id.0
        )));
    }
    if next.count < prev.count {
        return Err(AuditError::Mirror(format!(
            "refusing to move it back from {} entries to {}",
            prev.count, next.count
        )));
    }
    if next.count == prev.count && next.head_hash != prev.head_hash {
        return Err(AuditError::Mirror(format!(
            "refusing to replace its head at {} entries with another one",
            prev.count
        )));
    }
    Ok(())
}

/// Check a ledger against its mirrored checkpoint: the local checkpoint (or, for a
/// ledger that has none yet, its entry count) must not be behind the mirror, and the
/// ledger must hold the mirrored head at the mirrored position. Returns whether the
/// mirror is behind the ledger's local checkpoint. [MG-SEC-05]
pub fn check_against_mirror(
    mirrored: &LedgerCheckpoint,
    local: Option<&LedgerCheckpoint>,
    entries: &[SealedEntry],
    ledger_id: &LedgerId,
) -> Result<bool, AuditError> {
    if &mirrored.ledger_id != ledger_id {
        return Err(AuditError::Mirror(format!(
            "it holds ledger {:?}, not {:?}",
            mirrored.ledger_id.0, ledger_id.0
        )));
    }
    let local_count = local.map_or(entries.len() as u64, |c| c.count);
    if local_count < mirrored.count {
        return Err(AuditError::RolledBack {
            mirrored: mirrored.count,
            local: local_count,
        });
    }
    let head = match mirrored.count {
        0 => genesis_hash(ledger_id),
        n => entries
            .get((n - 1) as usize)
            .map(|e| e.entry_hash)
            .ok_or(AuditError::RolledBack {
                mirrored: n,
                local: entries.len() as u64,
            })?,
    };
    if head != mirrored.head_hash {
        return Err(AuditError::MirrorMismatch {
            seq: mirrored.count.saturating_sub(1),
        });
    }
    Ok(local.is_some_and(|c| c != mirrored))
}

/// A checkpoint mirror in a directory, meant to be on another mount than the ledger:
/// an NFS export, an object-store FUSE mount, a separate volume. Each ledger's latest
/// checkpoint is `<dir>/<ledger id>.checkpoint`, in [`LedgerCheckpoint::encode`]'s
/// signed form, replaced atomically. [MG-SEC-05]
///
/// The directory must already exist: an unmounted mount point is not quietly replaced
/// by a fresh local directory. One mirror directory serves one state directory —
/// two ledgers with the same id must not share one.
#[derive(Debug, Clone)]
pub struct FileCheckpointMirror {
    dir: PathBuf,
}

impl FileCheckpointMirror {
    /// A mirror in the existing directory `dir`.
    pub fn new(dir: impl AsRef<Path>) -> Result<Self, AuditError> {
        let dir = dir.as_ref().to_path_buf();
        if !dir.is_dir() {
            return Err(AuditError::Mirror(format!(
                "{} is not a directory (is the mirror volume mounted?)",
                dir.display()
            )));
        }
        Ok(Self { dir })
    }

    /// The mirror's directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where `ledger_id`'s checkpoint is kept. An id that is not a plain file name is
    /// hex-encoded.
    pub fn path_for(&self, ledger_id: &LedgerId) -> PathBuf {
        let id = &ledger_id.0;
        let plain = !id.is_empty()
            && !id.starts_with('.')
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        let name = if plain {
            id.clone()
        } else {
            id.bytes().fold(String::from("x-"), |mut s, b| {
                let _ = write!(s, "{b:02x}");
                s
            })
        };
        self.dir.join(format!("{name}.checkpoint"))
    }
}

fn mirror_io(path: &Path, e: std::io::Error) -> AuditError {
    AuditError::Mirror(format!("{}: {e}", path.display()))
}

impl CheckpointMirror for FileCheckpointMirror {
    fn latest(&self, ledger_id: &LedgerId) -> Result<Option<LedgerCheckpoint>, AuditError> {
        let path = self.path_for(ledger_id);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let cp = LedgerCheckpoint::decode(&bytes).map_err(|e| {
                    AuditError::Mirror(format!("{} is not a checkpoint: {e}", path.display()))
                })?;
                if &cp.ledger_id != ledger_id {
                    return Err(AuditError::Mirror(format!(
                        "{} holds ledger {:?}, not {:?}",
                        path.display(),
                        cp.ledger_id.0,
                        ledger_id.0
                    )));
                }
                Ok(Some(cp))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // A mirror that went away entirely is unreachable, not empty.
                if !self.dir.is_dir() {
                    return Err(AuditError::Mirror(format!(
                        "{} is not a directory (is the mirror volume mounted?)",
                        self.dir.display()
                    )));
                }
                Ok(None)
            }
            Err(e) => Err(mirror_io(&path, e)),
        }
    }

    fn publish(&self, checkpoint: &LedgerCheckpoint) -> Result<(), AuditError> {
        if let Some(prev) = self.latest(&checkpoint.ledger_id)? {
            check_forward(&prev, checkpoint)?;
            if &prev == checkpoint {
                return Ok(());
            }
        }
        let path = self.path_for(&checkpoint.ledger_id);
        write_atomic(&path, &checkpoint.encode()).map_err(|e| mirror_io(&path, e))
    }

    fn describe(&self, ledger_id: &LedgerId) -> String {
        self.path_for(ledger_id).display().to_string()
    }
}

/// Replace `target` atomically: write and `fsync` a temporary file beside it, rename
/// it over the old one, then `fsync` the directory, so a crash leaves either the old
/// content or the new, never a torn file. [MG-SEC-05]
pub(crate) fn write_atomic(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp_name = target.as_os_str().to_os_string();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    {
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, target)?;
    #[cfg(unix)]
    if let Some(dir) = target.parent() {
        // Make the rename itself durable.
        File::open(dir).and_then(|d| d.sync_all())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pqc_agility::{SigId, Signature};

    fn cp(count: u64, head: u8) -> LedgerCheckpoint {
        LedgerCheckpoint {
            ledger_id: LedgerId("L".into()),
            count,
            head_hash: [head; 32],
            sig_alg: SigId::MlDsa65,
            signature: Signature::new(SigId::MlDsa65, vec![head]),
        }
    }

    #[test]
    fn a_mirror_only_moves_forward() {
        assert!(check_forward(&cp(2, 1), &cp(3, 2)).is_ok());
        assert!(check_forward(&cp(2, 1), &cp(2, 1)).is_ok());
        assert!(check_forward(&cp(2, 1), &cp(1, 1)).is_err());
        assert!(check_forward(&cp(2, 1), &cp(2, 9)).is_err());
    }

    #[test]
    fn ledger_ids_map_to_plain_file_names() {
        let m = FileCheckpointMirror {
            dir: PathBuf::from("m"),
        };
        assert!(m
            .path_for(&LedgerId("midgard-governance".into()))
            .ends_with("midgard-governance.checkpoint"));
        assert!(m
            .path_for(&LedgerId("../x".into()))
            .ends_with("x-2e2e2f78.checkpoint"));
    }
}
