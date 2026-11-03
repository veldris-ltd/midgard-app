//! The fusion engine. Folds a stream of verified observations into a live picture:
//! correlate → track → layer → classify. Pure and deterministic — same observation
//! stream yields the same picture on any host, offline. [FR-H-1] [FR-H-2]

use heimdall_classify::Classifier;
use heimdall_feed_gateway::VerifiedObservation;
use heimdall_picture::{EntityId, Picture, Track, TrackedEntity};

use crate::layering::LayerPolicy;

/// Streaming fusion over the object fabric's picture. Correlation is by the
/// observation's `entity_key`: observations sharing a key — even across different
/// feeds — fuse into one [`TrackedEntity`]. An optional classifier labels each
/// entity as it updates (rules-based by default; Mimir-backed later, same seam).
#[derive(Default)]
pub struct FusionEngine {
    picture: Picture,
    layering: LayerPolicy,
    classifier: Option<Box<dyn Classifier>>,
}

impl FusionEngine {
    /// A new engine with the default layer policy and no classifier.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the layer-assignment policy.
    pub fn with_layering(mut self, layering: LayerPolicy) -> Self {
        self.layering = layering;
        self
    }

    /// Attach a classifier applied to every entity as it fuses. Absent → entities
    /// are tracked but unclassified (still a fully functional picture). [AC-H-6]
    pub fn with_classifier(mut self, classifier: Box<dyn Classifier>) -> Self {
        self.classifier = Some(classifier);
        self
    }

    /// The current picture.
    pub fn picture(&self) -> &Picture {
        &self.picture
    }

    /// Mutable access to the picture (e.g. for an operator's layer toggles).
    pub fn picture_mut(&mut self) -> &mut Picture {
        &mut self.picture
    }

    /// Fuse one verified observation, received at `received_at` by the caller's
    /// clock, returning the id of the entity it updated. Correlate by key, open or
    /// extend the track, (re)assign the layer, refresh the latest attributes for a
    /// newer observation, then classify. Which observation is newer is still decided
    /// by `observed_at`, so out-of-order feeds keep a monotone track; `received_at`
    /// is recorded beside it. The engine does no clock I/O and does not bound
    /// `observed_at`: the service that receives the observation does. [FR-H-1]
    /// [FR-H-2] [VLD-ANB-MIDGARD-0015]
    pub fn ingest(&mut self, obs: &VerifiedObservation, received_at: u64) -> EntityId {
        let o = obs.observation();
        let entity_id = EntityId(o.entity_key.clone());
        let (layer_id, layer) = self.layering.layer_for(&o.kind);
        self.picture.ensure_layer(layer);

        let mut entity = match self.picture.entity(&entity_id) {
            Some(existing) => {
                // Extend an existing track. `observe` keeps first/last monotone even
                // for out-of-order arrivals.
                let mut e = existing.clone();
                let is_newer = o.observed_at >= e.track.last_seen;
                e.track.observe(o.observed_at, received_at);
                e.sources.insert(o.feed_id.0.clone());
                if is_newer {
                    e.kind = o.kind.clone();
                    e.layer = layer_id.clone();
                    e.latest_attrs = o.attrs.clone();
                }
                e
            }
            None => {
                let mut sources = std::collections::BTreeSet::new();
                sources.insert(o.feed_id.0.clone());
                TrackedEntity {
                    id: entity_id.clone(),
                    entity_key: o.entity_key.clone(),
                    kind: o.kind.clone(),
                    layer: layer_id.clone(),
                    sources,
                    track: Track::opened_at(o.observed_at, received_at),
                    classification: None,
                    latest_attrs: o.attrs.clone(),
                }
            }
        };

        if let Some(clf) = &self.classifier {
            entity.classification = clf.classify(&entity);
        }

        self.picture.upsert_entity(entity);
        entity_id
    }

    /// Remove an entity from the picture, returning it if it was held.
    /// [VLD-ANB-MIDGARD-0017]
    pub fn remove(&mut self, id: &EntityId) -> Option<TrackedEntity> {
        self.picture.remove_entity(id)
    }

    /// Evict every entity not seen for at least `evict_after` seconds before `now`,
    /// returning their ids, oldest first. A picture fed over the network stays
    /// bounded by making room this way before a new entity is refused.
    /// [VLD-ANB-MIDGARD-0017]
    pub fn evict_stale(&mut self, now: u64, evict_after: u64) -> Vec<EntityId> {
        let stale: Vec<EntityId> = self
            .picture
            .stalest(self.picture.entity_count())
            .into_iter()
            .take_while(|id| {
                self.picture
                    .entity(id)
                    .is_some_and(|e| now.saturating_sub(e.last_seen()) >= evict_after)
            })
            .collect();
        for id in &stale {
            self.picture.remove_entity(id);
        }
        stale
    }

    /// Fuse a batch of verified observations in order, all received at
    /// `received_at`.
    pub fn ingest_all<'a, I>(&mut self, observations: I, received_at: u64)
    where
        I: IntoIterator<Item = &'a VerifiedObservation>,
    {
        for obs in observations {
            self.ingest(obs, received_at);
        }
    }
}
