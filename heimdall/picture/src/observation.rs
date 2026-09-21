//! Observations — one datum from a feed — and the small attribute value type they
//! carry. An observation is the unit a feed signs and the fusion engine correlates:
//! its `entity_key` is the correlation key that binds successive observations to the
//! same tracked entity.

use std::collections::BTreeMap;

use crate::codec;
use crate::id::{FeedId, ObservationId};

/// A typed attribute value on an observation or tracked entity. Deliberately small
/// and self-describing so it canonical-encodes deterministically. `Float` is why
/// this type is `PartialEq` but not `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub enum Attr {
    /// UTF-8 text.
    Text(String),
    /// 64-bit signed integer.
    Int(i64),
    /// 64-bit float (encoded by its IEEE-754 bit pattern).
    Float(f64),
    /// Boolean.
    Bool(bool),
}

/// One datum emitted by a feed. `observed_at` is a caller-supplied logical time
/// (this crate does no clock I/O — determinism and air-gap both depend on that).
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// This observation's id.
    pub id: ObservationId,
    /// The feed that emitted it.
    pub feed_id: FeedId,
    /// The observation kind (e.g. `"air-track"`, `"sigint"`) — drives layering.
    pub kind: String,
    /// The correlation key: observations sharing a key fuse to one entity. [FR-H-1]
    pub entity_key: String,
    /// Typed attributes, ordered so encoding is deterministic.
    pub attrs: BTreeMap<String, Attr>,
    /// Caller-supplied logical observation time.
    pub observed_at: u64,
}

impl Observation {
    /// Construct an observation with no attributes.
    pub fn new(
        id: impl Into<String>,
        feed_id: FeedId,
        kind: impl Into<String>,
        entity_key: impl Into<String>,
        observed_at: u64,
    ) -> Self {
        Self {
            id: ObservationId(id.into()),
            feed_id,
            kind: kind.into(),
            entity_key: entity_key.into(),
            attrs: BTreeMap::new(),
            observed_at,
        }
    }

    /// Builder: set one attribute.
    pub fn with_attr(mut self, key: impl Into<String>, value: Attr) -> Self {
        self.attrs.insert(key.into(), value);
        self
    }

    /// The canonical, deterministic byte encoding a feed source signs over and a
    /// verifier reproduces. Same observation → same bytes, on any host, offline.
    /// [FR-H-3] [NFR-PRV-1]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        codec::encode_observation(self)
    }

    /// Reconstruct an observation from its canonical bytes — the inverse of
    /// [`Observation::canonical_bytes`]. A receiver decodes the transmitted bytes
    /// and re-derives the same value to verify the feed signature. Fails closed on
    /// malformed input. [FR-H-3]
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, crate::error::PictureError> {
        codec::decode_observation(bytes)
    }
}
