//! Tracked entities and their temporal track + classification. A [`TrackedEntity`]
//! is the fused, over-time state the correlator/tracker maintain from many
//! observations sharing a correlation key. [FR-H-2]

use std::collections::{BTreeMap, BTreeSet};

use crate::id::{EntityId, LayerId};
use crate::observation::Attr;

/// The temporal state of an entity: when first/last seen, when an observation of it
/// was last received, and how many observations have fused into it. `last_seen` is
/// the merge priority during replication (last-write-wins), `last_received` its
/// tie-breaker. [FR-H-2] [FR-H-6]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Logical time of the first fused observation.
    pub first_seen: u64,
    /// Logical time of the most recent fused observation.
    pub last_seen: u64,
    /// How many observations have fused into this entity.
    pub observation_count: u64,
    /// When the receiving service last took in an observation of this entity, by
    /// its own clock: `observed_at` is what the feed asserts, this is what the
    /// service saw, so a track whose feed has gone quiet shows as stale.
    /// [VLD-ANB-MIDGARD-0015]
    pub last_received: u64,
}

impl Track {
    /// Open a track from its first observation, observed (by the feed) at
    /// `observed_at` and received (by the service) at `received_at`.
    pub fn opened_at(observed_at: u64, received_at: u64) -> Self {
        Self {
            first_seen: observed_at,
            last_seen: observed_at,
            observation_count: 1,
            last_received: received_at,
        }
    }

    /// Fold in a later observation observed at `observed_at` and received at
    /// `received_at`. `first_seen` only ever moves earlier and `last_seen` and
    /// `last_received` only ever move later, so out-of-order observations still
    /// yield a monotone, deterministic track.
    pub fn observe(&mut self, observed_at: u64, received_at: u64) {
        self.first_seen = self.first_seen.min(observed_at);
        self.last_seen = self.last_seen.max(observed_at);
        self.last_received = self.last_received.max(received_at);
        self.observation_count += 1;
    }
}

/// Where a classification came from. Phase-6.b ships `Rules`; 6.c adds `Mimir`
/// behind the same seam (advisory, deterministic gate decides). [FR-H-2]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationSource {
    /// Deterministic rules — the always-available, Mimir-absent path. [AC-H-6]
    Rules,
    /// Local Mimir inference (added in Prompt 6.c).
    Mimir,
}

impl ClassificationSource {
    /// Stable byte tag for canonical encoding.
    pub const fn as_u8(self) -> u8 {
        match self {
            ClassificationSource::Rules => 1,
            ClassificationSource::Mimir => 2,
        }
    }

    /// Decode a byte tag; unknown tags fail closed with `None`.
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(ClassificationSource::Rules),
            2 => Some(ClassificationSource::Mimir),
            _ => None,
        }
    }
}

/// A label applied to an entity, with its provenance and a human-readable
/// rationale. Written back with provenance so no classification is anonymous.
/// [FR-H-2]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    /// The label (e.g. `"fast-mover"`, `"surface-contact"`).
    pub label: String,
    /// Where the label came from.
    pub source: ClassificationSource,
    /// Why it was applied (which rule fired / which evidence).
    pub rationale: String,
}

/// A fused entity tracked over time and laid out on a picture layer. `latest_attrs`
/// holds the most recent observed attribute values (a compact current-state view;
/// full history lives in the feed/audit store, out of scope here).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackedEntity {
    /// Stable entity id.
    pub id: EntityId,
    /// The correlation key that fuses observations into this entity.
    pub entity_key: String,
    /// The entity kind, carried from its observations — drives layering.
    pub kind: String,
    /// The layer this entity is displayed on. [FR-H-1]
    pub layer: LayerId,
    /// The feed ids that have contributed observations to this entity — the
    /// provenance an option cites as `feed:<id>` evidence. [FR-H-4]
    pub sources: BTreeSet<String>,
    /// Temporal track state.
    pub track: Track,
    /// Current classification, if any has been applied. [FR-H-2]
    pub classification: Option<Classification>,
    /// Most-recent observed attributes.
    pub latest_attrs: BTreeMap<String, Attr>,
}

impl TrackedEntity {
    /// The entity's most recent logical time (its merge priority).
    pub fn last_seen(&self) -> u64 {
        self.track.last_seen
    }

    /// When an observation of the entity was last received (its merge tie-breaker).
    /// [VLD-ANB-MIDGARD-0015]
    pub fn last_received(&self) -> u64 {
        self.track.last_received
    }
}
