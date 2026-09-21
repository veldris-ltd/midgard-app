//! The feed trust registry: a mapping from [`FeedId`] to the post-quantum
//! verifying key that authenticates that source. A feed absent from the registry is
//! untrusted by construction — its observations are rejected. [FR-H-3]

use std::collections::BTreeMap;

use pqc_agility::VerifyingKey;

use heimdall_picture::FeedId;

/// The set of trusted feed sources and their verifying keys.
#[derive(Debug, Clone, Default)]
pub struct FeedRegistry {
    keys: BTreeMap<FeedId, VerifyingKey>,
}

impl FeedRegistry {
    /// An empty registry (no feeds trusted).
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a feed's verifying key.
    pub fn register(&mut self, feed: FeedId, key: VerifyingKey) {
        self.keys.insert(feed, key);
    }

    /// The verifying key for a feed, if it is trusted.
    pub fn key_for(&self, feed: &FeedId) -> Option<&VerifyingKey> {
        self.keys.get(feed)
    }

    /// Number of trusted feeds.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether no feeds are trusted.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}
