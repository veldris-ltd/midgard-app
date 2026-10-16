//! The common operating picture: fused entities laid out across layers, with
//! per-layer visibility toggles. This is the single shared view the console renders
//! and picture-sync replicates. [FR-H-1] [FR-H-6]

use std::collections::{BTreeMap, BTreeSet};

use crate::entity::TrackedEntity;
use crate::id::{EntityId, LayerId};
use crate::layer::Layer;

/// One live, layered operating picture. Layers default to visible; a layer id in
/// `hidden` is toggled off. Entities and layers are held in `BTreeMap`s so
/// iteration — and therefore any derived canonical bytes — is deterministic.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Picture {
    entities: BTreeMap<EntityId, TrackedEntity>,
    layers: BTreeMap<LayerId, Layer>,
    hidden: BTreeSet<LayerId>,
}

impl Picture {
    /// An empty picture.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a layer if absent (idempotent). Newly-registered layers are
    /// visible.
    pub fn ensure_layer(&mut self, layer: Layer) {
        self.layers.entry(layer.id.clone()).or_insert(layer);
    }

    /// Insert or replace a tracked entity, registering its layer if new.
    pub fn upsert_entity(&mut self, entity: TrackedEntity) {
        if !self.layers.contains_key(&entity.layer) {
            let id = entity.layer.clone();
            self.layers
                .insert(id.clone(), Layer::new(id.0.clone(), id.0));
        }
        self.entities.insert(entity.id.clone(), entity);
    }

    /// Borrow an entity by id.
    pub fn entity(&self, id: &EntityId) -> Option<&TrackedEntity> {
        self.entities.get(id)
    }

    /// Remove every entity last seen before `cutoff`, returning their ids in id order.
    /// Layers and their visibility stay, so an operator's toggle outlives the contacts
    /// that were on it. A replicated delta can bring an expired entity back: expiry is
    /// local, and picture-sync merges by `last_seen`. [FR-H-1] [FR-H-2]
    pub fn expire_before(&mut self, cutoff: u64) -> Vec<EntityId> {
        let stale: Vec<EntityId> = self
            .entities
            .values()
            .filter(|e| e.track.last_seen < cutoff)
            .map(|e| e.id.clone())
            .collect();
        for id in &stale {
            self.entities.remove(id);
        }
        stale
    }

    /// All entities, in id order — including those on hidden layers.
    pub fn entities(&self) -> impl Iterator<Item = &TrackedEntity> {
        self.entities.values()
    }

    /// Total entity count (all layers).
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// All registered layers, in id order.
    pub fn layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers.values()
    }

    /// Number of registered layers.
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Toggle a layer's visibility. A no-op for an unregistered layer other than
    /// recording the intent, so a later `ensure_layer` honours it.
    pub fn set_layer_visible(&mut self, layer: &LayerId, visible: bool) {
        if visible {
            self.hidden.remove(layer);
        } else {
            self.hidden.insert(layer.clone());
        }
    }

    /// Whether a layer is currently visible (unknown/absent → visible).
    pub fn is_layer_visible(&self, layer: &LayerId) -> bool {
        !self.hidden.contains(layer)
    }

    /// The entities an operator currently sees: those whose layer is visible, in id
    /// order. This is the per-layer toggle applied to the fused view. [FR-H-1]
    pub fn visible_entities(&self) -> impl Iterator<Item = &TrackedEntity> {
        self.entities
            .values()
            .filter(move |e| self.is_layer_visible(&e.layer))
    }

    /// Count of currently-visible entities.
    pub fn visible_count(&self) -> usize {
        self.visible_entities().count()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::entity::Track;

    fn entity(id: &str, last_seen: u64) -> TrackedEntity {
        TrackedEntity {
            id: EntityId(id.into()),
            entity_key: id.into(),
            kind: "air-track".into(),
            layer: LayerId("air".into()),
            sources: BTreeSet::new(),
            track: Track {
                first_seen: last_seen,
                last_seen,
                observation_count: 1,
            },
            classification: None,
            latest_attrs: BTreeMap::new(),
        }
    }

    #[test]
    fn expiry_removes_only_entities_last_seen_before_the_cutoff() {
        let mut picture = Picture::new();
        picture.upsert_entity(entity("OLD", 100));
        picture.upsert_entity(entity("EDGE", 200));
        picture.upsert_entity(entity("NEW", 300));
        picture.set_layer_visible(&LayerId("air".into()), false);

        assert_eq!(picture.expire_before(200), vec![EntityId("OLD".into())]);
        assert_eq!(picture.entity_count(), 2, "an entity at the cutoff stays");
        assert!(picture.entity(&EntityId("EDGE".into())).is_some());
        assert_eq!(picture.layer_count(), 1, "the layer outlives its contacts");
        assert!(!picture.is_layer_visible(&LayerId("air".into())));
        assert!(picture.expire_before(150).is_empty());
    }
}
