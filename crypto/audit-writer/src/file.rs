//! The durable, encrypted, append-only [`FileLedger`].
//!
//! Each sealed entry is encrypted at rest by a [`Sealer`] (the production impl is
//! KMS envelope encryption — AES-256-GCM under an ML-KEM-1024-wrapped key; see
//! `kms::KmsSealer`) and written as a length-framed record with `O_APPEND` +
//! per-entry `fsync` and `0600` permissions. On [`FileLedger::open`] every frame
//! is decrypted and decoded back into memory; any at-rest tamper fails the AEAD
//! authentication and the open fails closed. The caller then re-verifies the hash
//! chain + signatures with [`crate::verify::verify_chain`]. [AC-M-4] [NFR-PQ-3]

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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
}

impl FileLedger {
    /// Open (creating if absent) the ledger at `path`, decrypting and loading every
    /// existing entry into memory. Fails closed if any on-disk frame cannot be
    /// authenticated/decoded (at-rest tamper). After opening, the caller should
    /// re-verify the chain with [`crate::verify::verify_chain`].
    pub fn open(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        ledger_id: LedgerId,
    ) -> Result<Self, AuditError> {
        let path = path.as_ref().to_path_buf();

        let entries = if path.exists() {
            let mut buf = Vec::new();
            File::open(&path)
                .map_err(io)?
                .read_to_end(&mut buf)
                .map_err(io)?;
            load_frames(&buf, sealer.as_ref(), &ledger_id)?
        } else {
            Vec::new()
        };

        // Append-only handle, owner-read/write only.
        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        // On Windows the file inherits its folder's ACL: the app's working copy, %LOCALAPPDATA%\Midgard, is private to the user (the W2 shell checks it).
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&path).map_err(io)?;

        Ok(Self {
            path,
            file,
            sealer,
            ledger_id,
            entries,
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
}
