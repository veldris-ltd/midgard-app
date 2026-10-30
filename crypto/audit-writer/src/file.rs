//! The durable, encrypted, append-only [`FileLedger`].
//!
//! Each sealed entry is encrypted at rest by a [`Sealer`] (the production impl is
//! KMS envelope encryption — AES-256-GCM under an ML-KEM-1024-wrapped key; see
//! `kms::KmsSealer`) and written as a length-framed record with `O_APPEND` +
//! per-entry `fsync` and `0600` permissions. On [`FileLedger::open`] every frame
//! is decrypted and decoded back into memory; any at-rest tamper fails the AEAD
//! authentication and the open fails closed. The caller then re-verifies the hash
//! chain, signatures and checkpoint with [`crate::verify::verify_ledger`].
//! [AC-M-4] [NFR-PQ-3]
//!
//! Beside the ledger sits its signed [`LedgerCheckpoint`] (`<ledger>.checkpoint`),
//! replaced atomically after every append. [`FileLedger::open`] refuses a ledger
//! that is missing, has no checkpoint, or is shorter than its checkpoint; a new
//! ledger is made only by [`FileLedger::create`], so a deleted ledger is never
//! quietly replaced by an empty one. [MG-SEC-05]

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::checkpoint::LedgerCheckpoint;
use crate::codec::{decode_sealed_entry, encode_sealed_entry};
use crate::error::AuditError;
use crate::ledger::LedgerStore;
use crate::record::{LedgerId, SealedEntry};

/// Seals and unseals opaque bytes at rest. Kept backend-agnostic so `audit-writer`
/// does not depend on a KEM backend; the KMS binding lives in the `kms` crate.
///
/// `aad` is bound into the ciphertext (the ledger id + sequence), so a sealed
/// entry cannot be replayed at a different position or spliced into another ledger
/// without breaking authentication.
///
/// `Send + Sync` because a durable store or ledger holding a sealer sits behind a
/// service's shared state and is reached from its connection threads. [AC-M-4]
pub trait Sealer: Send + Sync {
    /// Seal `plaintext`, authenticating `aad`. Returns the self-describing sealed
    /// bytes to persist.
    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError>;
    /// Unseal bytes produced by [`Sealer::seal`] under the same `aad`. Fails closed
    /// on any tamper.
    fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError>;
}

const FRAME_DS: &[u8] = b"midgard-audit-file-v1";

/// AAD binding a sealed entry to its ledger and position (anti-splice/anti-reorder).
fn entry_aad(ledger: &LedgerId, seq: u64) -> Vec<u8> {
    let mut a = Vec::with_capacity(FRAME_DS.len() + 4 + ledger.0.len() + 8);
    a.extend_from_slice(FRAME_DS);
    a.extend_from_slice(&(ledger.0.len() as u32).to_le_bytes());
    a.extend_from_slice(ledger.0.as_bytes());
    a.extend_from_slice(&seq.to_le_bytes());
    a
}

fn io(e: std::io::Error) -> AuditError {
    AuditError::Store(e.to_string())
}

/// A durable, encrypted, append-only ledger store.
pub struct FileLedger {
    path: PathBuf,
    file: File,
    sealer: Box<dyn Sealer>,
    ledger_id: LedgerId,
    entries: Vec<SealedEntry>,
    checkpoint: Option<LedgerCheckpoint>,
}

/// Where a ledger's signed checkpoint lives: `<ledger path>.checkpoint`.
pub fn checkpoint_path(ledger: &Path) -> PathBuf {
    let mut name = ledger.as_os_str().to_os_string();
    name.push(".checkpoint");
    PathBuf::from(name)
}

fn append_handle(path: &Path, create_new: bool) -> Result<File, AuditError> {
    // Append-only handle, owner-read/write only.
    let mut opts = OpenOptions::new();
    opts.append(true);
    if create_new {
        opts.create_new(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path).map_err(io)
}

impl FileLedger {
    /// Create a new, empty ledger at `path`. Refuses if a ledger or a checkpoint is
    /// already there — an existing ledger is never overwritten. Until a first
    /// checkpoint is recorded (wrap it in an `AuditWriter` and call
    /// `seal_checkpoint`, or append) [`open`](Self::open) refuses it. [MG-SEC-05]
    pub fn create(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        ledger_id: LedgerId,
    ) -> Result<Self, AuditError> {
        let path = path.as_ref().to_path_buf();
        for p in [&path, &checkpoint_path(&path)] {
            if p.exists() {
                return Err(AuditError::LedgerExists(p.display().to_string()));
            }
        }
        let file = append_handle(&path, true)?;
        Ok(Self {
            path,
            file,
            sealer,
            ledger_id,
            entries: Vec::new(),
            checkpoint: None,
        })
    }

    /// Open the existing ledger at `path`, decrypting and loading every entry into
    /// memory. Fails closed if the ledger is missing ([`AuditError::LedgerMissing`]),
    /// its checkpoint is missing or malformed, any frame cannot be
    /// authenticated/decoded (at-rest tamper), or the ledger is shorter than, or
    /// diverges from, its checkpoint ([`AuditError::Truncated`]). The checkpoint's
    /// signature is not checked here — there is no key — so the caller must still
    /// re-verify with [`crate::verify::verify_ledger`]. [MG-SEC-05]
    pub fn open(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        ledger_id: LedgerId,
    ) -> Result<Self, AuditError> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(AuditError::LedgerMissing(path.display().to_string()));
        }
        let entries = read_ledger(&path, sealer.as_ref(), &ledger_id)?;
        let checkpoint = read_checkpoint(&path)?.ok_or(AuditError::CheckpointMissing)?;
        if checkpoint.ledger_id != ledger_id {
            return Err(AuditError::CheckpointInvalid(format!(
                "it is for ledger {:?}, not {:?}",
                checkpoint.ledger_id.0, ledger_id.0
            )));
        }
        checkpoint.check_covers(&entries)?;
        let file = append_handle(&path, false)?;
        Ok(Self {
            path,
            file,
            sealer,
            ledger_id,
            entries,
            checkpoint: Some(checkpoint),
        })
    }

    /// Open the ledger at `path`, or create it when **neither** the ledger nor its
    /// checkpoint exists (or the ledger holds no entry and was never checkpointed —
    /// a creation that stopped before its first checkpoint). Anything else follows
    /// [`open`](Self::open)'s rules, so a ledger without its checkpoint, or cut short
    /// of it, still fails closed.
    ///
    /// For stores that create their own ledger on first start and have no other
    /// record that one should exist; deleting both files is then indistinguishable
    /// from a first start. A ledger that an installer creates (the gateway's
    /// governance ledger) is opened with [`open`](Self::open) instead. [MG-SEC-05]
    pub fn open_or_create(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        ledger_id: LedgerId,
    ) -> Result<Self, AuditError> {
        let path = path.as_ref().to_path_buf();
        let has_checkpoint = checkpoint_path(&path).exists();
        if !path.exists() && !has_checkpoint {
            return Self::create(path, sealer, ledger_id);
        }
        if path.exists() && !has_checkpoint {
            let entries = read_ledger(&path, sealer.as_ref(), &ledger_id)?;
            if entries.is_empty() {
                let file = append_handle(&path, false)?;
                return Ok(Self {
                    path,
                    file,
                    sealer,
                    ledger_id,
                    entries,
                    checkpoint: None,
                });
            }
        }
        Self::open(path, sealer, ledger_id)
    }

    /// Open an existing ledger written before checkpoints existed, so it can be
    /// adopted: the caller verifies its chain under the pinned key and then records a
    /// first checkpoint (`AuditWriter::seal_checkpoint`). Refuses a missing ledger,
    /// and refuses one that already has a checkpoint — that one opens with
    /// [`open`](Self::open). An operator step, never a boot path. [MG-SEC-05]
    pub fn open_uncheckpointed(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        ledger_id: LedgerId,
    ) -> Result<Self, AuditError> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(AuditError::LedgerMissing(path.display().to_string()));
        }
        let checkpoint = checkpoint_path(&path);
        if checkpoint.exists() {
            return Err(AuditError::LedgerExists(checkpoint.display().to_string()));
        }
        let entries = read_ledger(&path, sealer.as_ref(), &ledger_id)?;
        let file = append_handle(&path, false)?;
        Ok(Self {
            path,
            file,
            sealer,
            ledger_id,
            entries,
            checkpoint: None,
        })
    }

    /// The ledger file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// The ledger id.
    pub fn ledger_id(&self) -> &LedgerId {
        &self.ledger_id
    }
}

fn read_ledger(
    path: &Path,
    sealer: &dyn Sealer,
    ledger_id: &LedgerId,
) -> Result<Vec<SealedEntry>, AuditError> {
    let mut buf = Vec::new();
    File::open(path)
        .map_err(io)?
        .read_to_end(&mut buf)
        .map_err(io)?;
    load_frames(&buf, sealer, ledger_id)
}

fn read_checkpoint(ledger: &Path) -> Result<Option<LedgerCheckpoint>, AuditError> {
    match std::fs::read(checkpoint_path(ledger)) {
        Ok(bytes) => LedgerCheckpoint::decode(&bytes).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(e)),
    }
}

/// Replace the checkpoint file atomically: write and `fsync` a temporary file, then
/// rename it over the old one, so a crash leaves either the old checkpoint or the
/// new one, never a torn one. [MG-SEC-05]
fn write_checkpoint(ledger: &Path, checkpoint: &LedgerCheckpoint) -> Result<(), AuditError> {
    let target = checkpoint_path(ledger);
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
        let mut f = opts.open(&tmp).map_err(io)?;
        f.write_all(&checkpoint.encode()).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    std::fs::rename(&tmp, &target).map_err(io)?;
    #[cfg(unix)]
    if let Some(dir) = target.parent() {
        // Make the rename itself durable.
        File::open(dir).and_then(|d| d.sync_all()).map_err(io)?;
    }
    Ok(())
}

/// Parse the length-framed file into decrypted, decoded entries. The positional
/// counter is the expected sequence: it feeds the AAD, so a reordered or spliced
/// frame fails AEAD authentication here.
fn load_frames(
    buf: &[u8],
    sealer: &dyn Sealer,
    ledger_id: &LedgerId,
) -> Result<Vec<SealedEntry>, AuditError> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut seq = 0u64;
    while pos < buf.len() {
        if pos + 4 > buf.len() {
            return Err(AuditError::Store("truncated frame header".into()));
        }
        let len = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]) as usize;
        pos += 4;
        let end = pos
            .checked_add(len)
            .ok_or_else(|| AuditError::Store("frame length overflow".into()))?;
        if end > buf.len() {
            return Err(AuditError::Store("truncated frame body".into()));
        }
        let sealed = &buf[pos..end];
        pos = end;

        let aad = entry_aad(ledger_id, seq);
        let plaintext = sealer.unseal(&aad, sealed)?; // AEAD auth: tamper -> fail closed
        out.push(decode_sealed_entry(&plaintext)?);
        seq += 1;
    }
    Ok(out)
}

impl LedgerStore for FileLedger {
    fn append(&mut self, entry: SealedEntry) -> Result<(), AuditError> {
        let aad = entry_aad(&self.ledger_id, entry.seq);
        let plaintext = encode_sealed_entry(&entry);
        let sealed = self.sealer.seal(&aad, &plaintext)?;

        let mut frame = Vec::with_capacity(4 + sealed.len());
        frame.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
        frame.extend_from_slice(&sealed);
        self.file.write_all(&frame).map_err(io)?;
        self.file.sync_all().map_err(io)?; // per-entry fsync: durable before we ack

        self.entries.push(entry);
        Ok(())
    }
    fn last(&self) -> Option<&SealedEntry> {
        self.entries.last()
    }
    fn entries(&self) -> &[SealedEntry] {
        &self.entries
    }
    fn record_checkpoint(&mut self, checkpoint: LedgerCheckpoint) -> Result<(), AuditError> {
        if checkpoint.ledger_id != self.ledger_id {
            return Err(AuditError::CheckpointInvalid(
                "it is for another ledger".into(),
            ));
        }
        write_checkpoint(&self.path, &checkpoint)?;
        self.checkpoint = Some(checkpoint);
        Ok(())
    }
    fn checkpoint(&self) -> Option<&LedgerCheckpoint> {
        self.checkpoint.as_ref()
    }
}
