//! Content addressing and version metadata. A dataset's identity **is** the
//! SHA-256 of its canonical bytes: two builds that produce the same data produce
//! the same [`ContentId`], which is exactly what makes rebuilds reproducible and
//! deduplicable. [FR-F-3] [AC-F-2]

use crate::lineage::Lineage;

/// Lowercase-hex encode (dependency-free), matching the suite's convention.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(LUT[(b >> 4) as usize] as char);
        s.push(LUT[(b & 0x0f) as usize] as char);
    }
    s
}

/// A content-addressed dataset identity: SHA-256 over the canonical dataset bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentId(pub [u8; 32]);

impl ContentId {
    /// The raw digest.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase-hex form (stable, for lineage and audit).
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }
}

/// Immutable metadata describing one committed dataset snapshot. The snapshot's
/// data lives sealed in the store; this record is its addressable, lineage-bearing
/// header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetVersion {
    /// Content id of this snapshot (SHA-256 of canonical bytes).
    pub content_id: ContentId,
    /// SHA-256 of the schema alone — a cheap "did the shape change?" check.
    pub schema_hash: [u8; 32],
    /// Parent snapshots this version was derived from (empty for a raw ingest).
    pub parents: Vec<ContentId>,
    /// How this version came to be (source/operation/inputs). [NFR-SEC-2]
    pub lineage: Lineage,
    /// Row count at commit time.
    pub row_count: usize,
}
