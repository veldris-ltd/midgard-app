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
