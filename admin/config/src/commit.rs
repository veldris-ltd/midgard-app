//! A signed configuration commit and the domain-separated preimage it signs.

use pqc_agility::{SigId, Signature};

use crate::state::ConfigState;

/// Domain separator so a config-commit signature can never be confused with an
/// audit-entry, token, or artefact signature.
const DS_COMMIT: &[u8] = b"midgard-admin-config-commit-v1";

/// One version in the config history: a content-hashed, parent-linked, PQ-signed
/// snapshot. `[FR-AD-7]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Monotonic version number (0 = genesis).
    pub version: u64,
    /// Content hash of the parent commit (`None` at genesis) — the append-only
    /// linkage that makes the history tamper-evident.
    pub parent: Option<[u8; 32]>,
    /// Content hash of this commit's state.
    pub content_hash: [u8; 32],
    /// Who made the change.
    pub author: String,
    /// The change message.
    pub message: String,
    /// The full snapshot at this version.
    pub state: ConfigState,
    /// Algorithm of `signature`.
    pub sig_alg: SigId,
    /// ML-DSA / SLH-DSA signature over [`commit_preimage`]. `[NFR-PQ-3]`
    pub signature: Signature,
}

/// The exact bytes a commit signs: domain tag ‖ version ‖ parent ‖ content hash ‖
/// author ‖ message. Length-prefixed so it is injective.
pub fn commit_preimage(
    version: u64,
    parent: &Option<[u8; 32]>,
    content_hash: &[u8; 32],
    author: &str,
    message: &str,
) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(DS_COMMIT);
    v.extend_from_slice(&version.to_le_bytes());
    match parent {
        None => v.push(0),
        Some(h) => {
            v.push(1);
            v.extend_from_slice(h);
        }
    }
    v.extend_from_slice(content_hash);
    v.extend_from_slice(&(author.len() as u32).to_le_bytes());
    v.extend_from_slice(author.as_bytes());
    v.extend_from_slice(&(message.len() as u32).to_le_bytes());
    v.extend_from_slice(message.as_bytes());
    v
}
