//! Identifier newtypes. Each is a thin, ordered wrapper over a `String` so ids are
//! never confused positionally and so entities/layers key deterministically in the
//! picture's `BTreeMap`s (stable iteration → stable canonical bytes).

/// Identifies a feed (a PQ-authenticated source). [FR-H-3]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FeedId(pub String);

/// Identifies a single observation emitted by a feed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObservationId(pub String);

/// Identifies a fused entity tracked over time. [FR-H-2]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub String);

/// Identifies a display/organisational layer of the picture. [FR-H-1]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId(pub String);

impl FeedId {
    /// The underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl EntityId {
    /// The underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl LayerId {
    /// The underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
