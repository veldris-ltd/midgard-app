//! The feed trust registry: a mapping from [`FeedId`] to the post-quantum
//! verifying key that authenticates that source. A feed absent from the registry is
//! untrusted by construction — its observations are rejected. [FR-H-3]
//!
//! A feed's observations arrive from the PQ-mTLS peer whose service id is the feed's
//! id, or from a peer the registry delegates the feed to (a relay forwarding another
//! source's signed observations). [VLD-ANB-MIDGARD-0041]

use std::collections::{BTreeMap, BTreeSet};

use pq_mtls::ServiceId;
use pqc_agility::VerifyingKey;

use heimdall_picture::FeedId;

/// The set of trusted feed sources and their verifying keys, and which other peers
/// may submit each feed's observations.
#[derive(Debug, Clone, Default)]
pub struct FeedRegistry {
    keys: BTreeMap<FeedId, VerifyingKey>,
    delegations: BTreeMap<FeedId, BTreeSet<ServiceId>>,
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

    /// Let the PQ-mTLS peer `peer` submit `feed`'s signed observations, as well as the
    /// peer whose service id is the feed's own. The signature is still checked
    /// against the feed's key. [VLD-ANB-MIDGARD-0041]
    pub fn delegate(&mut self, peer: ServiceId, feed: FeedId) {
        self.delegations.entry(feed).or_default().insert(peer);
    }

    /// Whether `peer` may submit `feed`'s observations: it is the feed's own service,
    /// or the feed is delegated to it. [VLD-ANB-MIDGARD-0041]
    pub fn may_submit(&self, peer: &ServiceId, feed: &FeedId) -> bool {
        feed.0 == peer.0
            || self
                .delegations
                .get(feed)
                .is_some_and(|peers| peers.contains(peer))
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
