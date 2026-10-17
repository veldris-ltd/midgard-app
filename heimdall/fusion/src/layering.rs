//! Layer assignment. Every entity is placed on exactly one layer, derived from its
//! observation kind, so operators can toggle a whole class of contacts in or out of
//! the fused view. Assignment is deterministic and configurable. [FR-H-1]

use std::collections::BTreeMap;

use heimdall_picture::{Layer, LayerId};

/// Maps an observation/entity kind to the layer it belongs on. A default prefix
/// convention (`air-*`, `sea-*`, `land-*`, `space-*`, `cyber-*`/`sigint`, `event-*`) covers
/// common feeds; explicit overrides take precedence.
#[derive(Debug, Clone, Default)]
pub struct LayerPolicy {
    overrides: BTreeMap<String, (String, String)>,
}

impl LayerPolicy {
    /// The default prefix-based policy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Force a specific kind onto a named layer, overriding the prefix convention.
    pub fn with_override(
        mut self,
        kind: impl Into<String>,
        layer_id: impl Into<String>,
        layer_name: impl Into<String>,
    ) -> Self {
        self.overrides
            .insert(kind.into(), (layer_id.into(), layer_name.into()));
        self
    }

    /// The layer for a kind: its id plus a [`Layer`] to register.
    pub fn layer_for(&self, kind: &str) -> (LayerId, Layer) {
        if let Some((id, name)) = self.overrides.get(kind) {
            return (LayerId(id.clone()), Layer::new(id.clone(), name.clone()));
        }
        let (id, name) = match domain_of(kind) {
            "air" => ("air", "Air"),
            "sea" => ("sea", "Maritime"),
            "land" => ("land", "Land"),
            "space" => ("space", "Space"),
            "cyber" => ("cyber", "Cyber"),
            "events" => ("events", "Events"),
            _ => ("other", "Other"),
        };
        (LayerId(id.to_string()), Layer::new(id, name))
    }
}

/// Derive a coarse domain from a kind string.
fn domain_of(kind: &str) -> &'static str {
    let k = kind.to_ascii_lowercase();
    if k.starts_with("air") {
        "air"
    } else if k.starts_with("sea") || k.starts_with("mar") || k.starts_with("surface") {
        "sea"
    } else if k.starts_with("land") || k.starts_with("ground") {
        "land"
    } else if k.starts_with("space") || k.starts_with("orbit") {
        "space"
    } else if k.starts_with("cyber") || k.starts_with("sigint") || k.starts_with("net") {
        "cyber"
    } else if k.starts_with("event") {
        // Incidents and alerts (`event-*`) are their own layer. [FR-H-1]
        "events"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_convention() {
        let p = LayerPolicy::new();
        assert_eq!(p.layer_for("air-track").0, LayerId("air".into()));
        assert_eq!(p.layer_for("sea-track").0, LayerId("sea".into()));
        assert_eq!(p.layer_for("sigint").0, LayerId("cyber".into()));
        assert_eq!(p.layer_for("weather").0, LayerId("other".into()));
    }

    #[test]
    fn events_have_their_own_layer() {
        let p = LayerPolicy::new();
        let (id, layer) = p.layer_for("event-incident");
        assert_eq!(id, LayerId("events".into()));
        assert_eq!(layer.name, "Events");
        assert_eq!(p.layer_for("Event").0, LayerId("events".into()));
        assert_eq!(p.layer_for("eventual").0, LayerId("events".into()));
    }

    #[test]
    fn override_wins() {
        let p = LayerPolicy::new().with_override("air-track", "special", "Special");
        assert_eq!(p.layer_for("air-track").0, LayerId("special".into()));
    }
}
