//! A configuration snapshot: a canonical, deterministically-hashable key/value map.

use std::collections::BTreeMap;

use pqc_agility::sha256;

/// A configuration snapshot. `BTreeMap` so iteration — and therefore the content
/// hash — is deterministic regardless of insertion order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigState {
    entries: BTreeMap<String, String>,
}

impl ConfigState {
    /// An empty snapshot.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Set a key (builder-style), returning the snapshot for chaining.
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.entries.insert(key.into(), value.into());
        self
    }

    /// Set a key in place.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }

    /// Remove a key in place; returns the removed value if present.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.entries.remove(key)
    }

    /// Look up a key.
    pub fn get(&self, key: &str) -> Option<&String> {
        self.entries.get(key)
    }

    /// The underlying entries (sorted).
    pub fn entries(&self) -> &BTreeMap<String, String> {
        &self.entries
    }

    /// Injective canonical encoding (length-prefixed, sorted) for content hashing.
    pub fn canonical(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for (k, v) in &self.entries {
            out.extend_from_slice(&(k.len() as u32).to_le_bytes());
            out.extend_from_slice(k.as_bytes());
            out.extend_from_slice(&(v.len() as u32).to_le_bytes());
            out.extend_from_slice(v.as_bytes());
        }
        out
    }

    /// The SHA-256 content hash of this snapshot.
    pub fn content_hash(&self) -> [u8; 32] {
        sha256(&self.canonical())
    }
}
