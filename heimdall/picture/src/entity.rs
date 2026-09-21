//! Tracked entities and their temporal track + classification. A [`TrackedEntity`]
//! is the fused, over-time state the correlator/tracker maintain from many
//! observations sharing a correlation key. [FR-H-2]

use std::collections::{BTreeMap, BTreeSet};

use crate::id::{EntityId, LayerId};
use crate::observation::Attr;

/// The temporal state of an entity: when first/last seen and how many observations
/// have fused into it. `last_seen` is the merge tie-breaker during replication
/// (last-write-wins). [FR-H-2] [FR-H-6]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Logical time of the first fused observation.
    pub first_seen: u64,
    /// Logical time of the most recent fused observation.
    pub last_seen: u64,
    /// How many observations have fused into this entity.
    pub observation_count: u64,
}

impl Track {
    /// Open a track from its first observation time.
    pub fn opened_at(t: u64) -> Self {
        Self {
            first_seen: t,
            last_seen: t,
            observation_count: 1,
        }
    }

    /// Fold in a later observation at logical time `t`. `first_seen` only ever
    /// moves earlier and `last_seen` only ever moves later, so out-of-order
    /// observations still yield a monotone, deterministic track.
    pub fn observe(&mut self, t: u64) {
        self.first_seen = self.first_seen.min(t);
        self.last_seen = self.last_seen.max(t);
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
}
