//! Shared helpers: atomic file writes and a fail-closed byte reader. [FR-H-5]

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::error::StoreError;

/// Write `bytes` to `path` atomically: sibling temp file, `fsync`, then rename.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let tmp = path.with_extension("tmp");
    {
        let mut opts = OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f: File = opts.open(&tmp).map_err(io)?;
        f.write_all(bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    fs::rename(&tmp, path).map_err(io)?;
    Ok(())
}

pub(crate) fn io(e: std::io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

pub(crate) fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

/// A bounds-checked reader over a byte slice; every short read fails closed.
pub(crate) struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], StoreError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| StoreError::Decode("truncated".into()))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, StoreError> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn u32(&mut self) -> Result<usize, StoreError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    pub(crate) fn i32(&mut self) -> Result<i32, StoreError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub(crate) fn u64(&mut self) -> Result<u64, StoreError> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    pub(crate) fn bytes(&mut self) -> Result<&'a [u8], StoreError> {
        let n = self.u32()?;
        self.take(n)
    }
    pub(crate) fn string(&mut self) -> Result<String, StoreError> {
        String::from_utf8(self.bytes()?.to_vec())
            .map_err(|_| StoreError::Decode("invalid utf8".into()))
    }
    pub(crate) fn done(&self) -> bool {
        self.pos == self.b.len()
    }
}
