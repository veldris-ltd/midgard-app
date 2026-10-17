//! The fusion engine. Folds a stream of verified observations into a live picture:
//! correlate → track → layer → classify. Pure and deterministic — same observation
//! stream yields the same picture on any host, offline. [FR-H-1] [FR-H-2]
//!
//! ## Position quality
//! An observation may say where its position came from in a text attribute
//! [`POS_SOURCE`] (`adsb_icao`, `mlat`, `tisb_icao`, `mode_s`, ...). An ADS-B position
//! (a source starting `adsb`) is the aircraft's own GNSS fix; anything else is derived.
//! So while an entity's current position came from ADS-B observed within
//! [`ADSB_PREFERENCE_SECS`] (observation time), a newer observation whose position is
//! *not* ADS-B keeps the entity's `lat`, `lon`, `speed` and `track` — its other
//! attributes still merge, it still counts and its feed is still a source. The entity
//! records who supplied its current position in [`POS_FEED`] (and [`POS_SOURCE`]) and
//! when in [`POS_AT`]. A position with no `pos_source` is adopted as before. [FR-H-1]
//! [FR-H-2]

use std::collections::BTreeMap;

use heimdall_classify::Classifier;
use heimdall_feed_gateway::VerifiedObservation;
use heimdall_picture::{Attr, EntityId, Picture, Track, TrackedEntity};

/// The attribute naming where an observation's position came from.
pub const POS_SOURCE: &str = "pos_source";
/// The attribute naming the feed that supplied the entity's current position.
pub const POS_FEED: &str = "pos_feed";
/// The attribute holding the observation time of the entity's current position.
pub const POS_AT: &str = "pos_at";
/// How long, in seconds of observation time, an ADS-B position outranks a derived one.
pub const ADSB_PREFERENCE_SECS: u64 = 60;
/// The attributes a held ADS-B position keeps from its own source.
const HELD: [&str; 7] = ["lat", "lon", "speed", "track", POS_SOURCE, POS_FEED, POS_AT];

/// What one fused observation did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fused {
    /// The entity it updated.
    pub id: EntityId,
    /// Whether it carried a position that was taken as a point on the track: false for
    /// no position, and for a derived position an ADS-B one outranked.
    pub position_adopted: bool,
}

fn has_position(attrs: &BTreeMap<String, Attr>) -> bool {
    let num = |k: &str| matches!(attrs.get(k), Some(Attr::Float(_)) | Some(Attr::Int(_)));
    num("lat") && num("lon")
}

fn text<'a>(attrs: &'a BTreeMap<String, Attr>, key: &str) -> Option<&'a str> {
    match attrs.get(key) {
        Some(Attr::Text(t)) => Some(t),
        _ => None,
    }
}

fn is_adsb(source: &str) -> bool {
    source.starts_with("adsb")
}

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

    /// Fuse one verified observation, returning the id of the entity it updated.
    /// Correlate by key, open or extend the track, (re)assign the layer, refresh
    /// the latest attributes for a newer observation, then classify. [FR-H-1]
    /// [FR-H-2]
    pub fn ingest(&mut self, obs: &VerifiedObservation) -> EntityId {
        self.fuse(obs).id
    }

    /// [`FusionEngine::ingest`], saying also whether the observation's position was
    /// adopted (see the module docs on position quality). [FR-H-1]
    pub fn fuse(&mut self, obs: &VerifiedObservation) -> Fused {
        let o = obs.observation();
        let positioned = has_position(&o.attrs);
        // The observation's own position, stamped with who supplied it and when; a
        // feed cannot claim another's `pos_feed`.
        let mut attrs = o.attrs.clone();
        attrs.remove(POS_FEED);
        attrs.remove(POS_AT);
        if positioned {
            attrs.insert(POS_FEED.into(), Attr::Text(o.feed_id.0.clone()));
            attrs.insert(
                POS_AT.into(),
                Attr::Int(i64::try_from(o.observed_at).unwrap_or(i64::MAX)),
            );
        }
        let mut position_adopted = positioned;
        let entity_id = EntityId(o.entity_key.clone());
        let (layer_id, layer) = self.layering.layer_for(&o.kind);
        self.picture.ensure_layer(layer);

        let mut entity = match self.picture.entity(&entity_id) {
            Some(existing) => {
                // Extend an existing track. `observe` keeps first/last monotone even
                // for out-of-order arrivals.
                let mut e = existing.clone();
                let is_newer = o.observed_at >= e.track.last_seen;
                // A derived position near in time to the entity's ADS-B one is not
                // taken: not as the current position, nor as a point on the track.
                let held = positioned
                    && text(&o.attrs, POS_SOURCE).is_some_and(|s| !is_adsb(s))
                    && has_position(&e.latest_attrs)
                    && text(&e.latest_attrs, POS_SOURCE).is_some_and(is_adsb)
                    && match e.latest_attrs.get(POS_AT) {
                        Some(Attr::Int(at)) => {
                            let at = u64::try_from(*at).unwrap_or(0);
                            o.observed_at.abs_diff(at) <= ADSB_PREFERENCE_SECS
                        }
                        _ => false,
                    };
                if held {
                    position_adopted = false;
                    for key in HELD {
                        match e.latest_attrs.get(key) {
                            Some(v) => attrs.insert(key.to_string(), v.clone()),
                            None => attrs.remove(key),
                        };
                    }
                }
                e.track.observe(o.observed_at);
                e.sources.insert(o.feed_id.0.clone());
                if is_newer {
                    e.kind = o.kind.clone();
                    e.layer = layer_id.clone();
                    e.latest_attrs = attrs;
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
                    track: Track::opened_at(o.observed_at),
                    classification: None,
                    latest_attrs: attrs,
                }
            }
        };

        if let Some(clf) = &self.classifier {
            entity.classification = clf.classify(&entity);
        }

        self.picture.upsert_entity(entity);
        Fused {
            id: entity_id,
            position_adopted,
        }
    }

    /// Drop every entity last seen before `cutoff` from the live picture, returning the
    /// removed ids. The caller decides the cutoff (now minus a track time-to-live), so
    /// the engine itself stays clock-free. [FR-H-2] [AC-H-6]
    pub fn expire_before(&mut self, cutoff: u64) -> Vec<EntityId> {
        self.picture.expire_before(cutoff)
    }

    /// Fuse a batch of verified observations in order.
    pub fn ingest_all<'a, I>(&mut self, observations: I)
    where
        I: IntoIterator<Item = &'a VerifiedObservation>,
    {
        for obs in observations {
            self.ingest(obs);
        }
    }
}
