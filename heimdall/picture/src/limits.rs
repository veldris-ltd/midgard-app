//! Picture bounds. A picture fed by sources over the network must not grow without
//! limit: [`PictureLimits`] caps how many entities it holds, overall and per feed,
//! and how many attributes (and how many bytes each) an observation may carry, and
//! says how long a quiet track is kept before it may be evicted to make room.
//! [FR-H-1] [VLD-ANB-MIDGARD-0017]

use std::collections::BTreeMap;

use crate::observation::{Attr, Observation};

/// The bounds a fed picture is held to. A new entity that would pass a cap first
/// evicts tracks quiet for at least `evict_after`; if none is, it is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureLimits {
    /// Entities the picture holds at most.
    pub max_entities: usize,
    /// Entities one feed may have contributed to at most.
    pub max_entities_per_feed: usize,
    /// Attributes one observation (and so one entity) may carry at most.
    pub max_attrs_per_entity: usize,
    /// Encoded bytes one attribute (key and value) may take at most; an
    /// observation's entity key and kind are held to the same bound.
    pub max_attr_bytes: usize,
    /// Seconds since an entity was last seen after which it may be evicted.
    pub evict_after: u64,
}

impl Default for PictureLimits {
    /// 50,000 entities, 10,000 per feed, 64 attributes of at most 4,096 bytes, and a
    /// track quiet for 7 days may be evicted.
    fn default() -> Self {
        Self {
            max_entities: 50_000,
            max_entities_per_feed: 10_000,
            max_attrs_per_entity: 64,
            max_attr_bytes: 4_096,
            evict_after: 604_800,
        }
    }
}

impl PictureLimits {
    /// Whether `observation` is within the attribute caps, and its entity key and
    /// kind within `max_attr_bytes`; the error says which is passed.
    pub fn check_observation(&self, observation: &Observation) -> Result<(), String> {
        for (what, value) in [
            ("entity key", &observation.entity_key),
            ("kind", &observation.kind),
        ] {
            if value.len() > self.max_attr_bytes {
                return Err(format!(
                    "the {what} takes {} bytes, more than the {} allowed",
                    value.len(),
                    self.max_attr_bytes
                ));
            }
        }
        self.check_attrs(&observation.attrs)
    }

    /// Whether `attrs` are within the attribute caps; the error says which is passed.
    pub fn check_attrs(&self, attrs: &BTreeMap<String, Attr>) -> Result<(), String> {
        if attrs.len() > self.max_attrs_per_entity {
            return Err(format!(
                "the observation carries {} attributes, more than the {} allowed",
                attrs.len(),
                self.max_attrs_per_entity
            ));
        }
        for (key, value) in attrs {
            let size = attr_size(key, value);
            if size > self.max_attr_bytes {
                return Err(format!(
                    "attribute '{}' takes {size} bytes, more than the {} allowed",
                    truncated(key),
                    self.max_attr_bytes
                ));
            }
        }
        Ok(())
    }
}

/// The bytes an attribute takes: its key and its value as encoded.
pub fn attr_size(key: &str, value: &Attr) -> usize {
    key.len()
        + match value {
            Attr::Text(t) => t.len(),
            Attr::Int(_) | Attr::Float(_) => 8,
            Attr::Bool(_) => 1,
        }
}

/// At most 64 characters of `key`, for an error message.
fn truncated(key: &str) -> String {
    key.chars().take(64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_caps_count_and_size() {
        let limits = PictureLimits::default();
        let mut attrs = BTreeMap::new();
        for i in 0..64 {
            attrs.insert(format!("a{i}"), Attr::Int(i));
        }
        assert!(limits.check_attrs(&attrs).is_ok());
        attrs.insert("a64".into(), Attr::Int(64));
        assert!(limits.check_attrs(&attrs).is_err());

        let mut big = BTreeMap::new();
        big.insert("k".to_string(), Attr::Text("x".repeat(4_095)));
        assert!(limits.check_attrs(&big).is_ok());
        big.insert("k".to_string(), Attr::Text("x".repeat(4_096)));
        assert!(limits.check_attrs(&big).is_err());

        let long_key =
            Observation::new("o", crate::FeedId("f".into()), "air", "k".repeat(4_097), 1);
        assert!(limits.check_observation(&long_key).is_err());
        let fine = Observation::new("o", crate::FeedId("f".into()), "air", "k", 1);
        assert!(limits.check_observation(&fine).is_ok());
    }
}
