//! A generic, durable, KMS-sealed append-only journal of opaque byte records.
//!
//! The reusable core of every "persist a store's records, recover on boot" durable
//! store (fabric/fleet did this inline; the admin stores share this one). Each
//! record is framed (`u32` length + sealed bytes), encrypted-and-authenticated at
//! rest by a [`Sealer`], `fsync`ed, with `0600` perms; the sequence number is bound
//! into the AAD so a reordered or spliced record fails authentication. On
//! [`SealedJournal::open`] every record is decrypted and returned in order (a
//! tampered record fails closed); the caller decodes and replays them into its
//! in-memory state. [AC-M-4] [RC-14]

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::AuditError;
use crate::file::Sealer;

/// A durable, encrypted append-only log of opaque records.
pub struct SealedJournal {
    file: File,
    path: PathBuf,
    sealer: Box<dyn Sealer>,
    domain: Vec<u8>,
    seq: u64,
}

impl SealedJournal {
    /// Open (creating if absent) the journal at `path`, returning it plus every
    /// recovered **plaintext** record in append order. `domain` namespaces the AAD
    /// so records of one store can never be replayed into another. Fails closed on
    /// a tampered/corrupt record.
    pub fn open(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        domain: &[u8],
    ) -> Result<(Self, Vec<Vec<u8>>), AuditError> {
        let path = path.as_ref().to_path_buf();
        let domain = domain.to_vec();
        let mut records = Vec::new();
        let mut seq = 0u64;

        if path.exists() {
            let mut buf = Vec::new();
            File::open(&path)
                .map_err(io)?
                .read_to_end(&mut buf)
                .map_err(io)?;
            let mut pos = 0usize;
            while pos < buf.len() {
                if pos + 4 > buf.len() {
                    return Err(AuditError::Store("truncated frame header".into()));
                }
                let len = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
                    as usize;
                pos += 4;
                let end = pos
                    .checked_add(len)
                    .filter(|&e| e <= buf.len())
                    .ok_or_else(|| AuditError::Store("truncated frame body".into()))?;
                let plaintext = sealer.unseal(&aad(&domain, seq), &buf[pos..end])?;
                pos = end;
                records.push(plaintext);
                seq += 1;
            }
        }

        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&path).map_err(io)?;

        Ok((
            Self {
                file,
                path,
                sealer,
                domain,
                seq,
            },
            records,
        ))
    }

    /// Seal, frame, and `fsync` one opaque record.
    pub fn append(&mut self, record: &[u8]) -> Result<(), AuditError> {
        let sealed = self.sealer.seal(&aad(&self.domain, self.seq), record)?;
        let mut frame = Vec::with_capacity(4 + sealed.len());
        frame.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
        frame.extend_from_slice(&sealed);
        self.file.write_all(&frame).map_err(io)?;
        self.file.sync_all().map_err(io)?;
        self.seq += 1;
        Ok(())
    }

    /// The number of records written so far.
    pub fn len(&self) -> u64 {
        self.seq
    }
    /// Whether the journal is empty.
    pub fn is_empty(&self) -> bool {
        self.seq == 0
    }
    /// The journal path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn aad(domain: &[u8], seq: u64) -> Vec<u8> {
    let mut a = Vec::with_capacity(domain.len() + 8);
    a.extend_from_slice(domain);
    a.extend_from_slice(&seq.to_le_bytes());
    a
}

fn io(e: std::io::Error) -> AuditError {
    AuditError::Store(e.to_string())
}
