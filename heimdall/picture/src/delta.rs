//! Picture deltas — the unit of cross-node replication. A [`PictureDelta`] is a set
//! of upserted entities plus the layers they need, canonical-encoded so a peer node
//! can verify a post-quantum signature over it and merge it deterministically.
//! [FR-H-6]

use std::cmp::Ordering;

use crate::codec;
use crate::entity::TrackedEntity;
use crate::error::PictureError;
use crate::layer::Layer;
use crate::picture::Picture;

/// A replicable slice of a picture: the entities to upsert and the layers they
/// belong to. Merge is **last-write-wins by `last_seen`**, ties broken by
/// `last_received` and then by canonical bytes, so it is commutative and idempotent
/// and two nodes exchanging deltas in any order converge. [FR-H-6]
/// [VLD-ANB-MIDGARD-0015]
#[derive(Debug, Clone, PartialEq)]
pub struct PictureDelta {
    /// Layers referenced by the entities in this delta.
    pub layers: Vec<Layer>,
    /// Entities to upsert.
    pub entities: Vec<TrackedEntity>,
}

impl PictureDelta {
    /// A full snapshot of `picture` as a delta (every entity + layer). Layer
    /// visibility toggles are a local operator concern and are intentionally not
    /// replicated.
    pub fn snapshot(picture: &Picture) -> Self {
        Self {
            layers: picture.layers().cloned().collect(),
            entities: picture.entities().cloned().collect(),
        }
    }

    /// Canonical bytes — the signing/verification preimage for replication.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        codec::encode_delta(self)
    }

    /// Decode canonical bytes back into a delta, failing closed on malformed input.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, PictureError> {
        codec::decode_delta(bytes)
    }

    /// Merge this delta into `picture` with last-write-wins by `last_seen`: an
    /// incoming entity replaces the local one when it is newer. On equal `last_seen`
    /// the larger `last_received` wins, and on a full tie the entity whose canonical
    /// bytes compare greater, so whichever order two nodes merge in, both keep the
    /// same entity. Returns how many entities were newly inserted or updated.
    /// [VLD-ANB-MIDGARD-0015]
    pub fn merge_into(&self, picture: &mut Picture) -> usize {
        for layer in &self.layers {
            picture.ensure_layer(layer.clone());
        }
        let mut changed = 0;
        for incoming in &self.entities {
            let keep_local = picture
                .entity(&incoming.id)
                .is_some_and(|local| prefer_local(local, incoming));
            if !keep_local {
                picture.upsert_entity(incoming.clone());
                changed += 1;
            }
        }
        changed
    }
}

/// Whether `local` wins over `incoming` for the same entity id: a total order on
/// (`last_seen`, `last_received`, canonical bytes), so the merge converges. An
/// identical incoming entity replaces the local one, which changes nothing.
fn prefer_local(local: &TrackedEntity, incoming: &TrackedEntity) -> bool {
    match (local.last_seen(), local.last_received())
        .cmp(&(incoming.last_seen(), incoming.last_received()))
    {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => codec::encode_entity(local) > codec::encode_entity(incoming),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::entity::Track;
    use crate::id::{EntityId, LayerId};
    use crate::observation::Attr;

    fn entity(feed: &str, speed: i64, last_seen: u64, last_received: u64) -> TrackedEntity {
        TrackedEntity {
            id: EntityId("AC-1".into()),
            entity_key: "AC-1".into(),
            kind: "air-track".into(),
            layer: LayerId("air".into()),
            sources: BTreeSet::from([feed.to_string()]),
            track: Track {
                first_seen: 100,
                last_seen,
                observation_count: 1,
                last_received,
            },
            classification: None,
            latest_attrs: BTreeMap::from([("speed".to_string(), Attr::Int(speed))]),
        }
    }

    fn picture_of(entity: TrackedEntity) -> Picture {
        let mut picture = Picture::new();
        PictureDelta {
            layers: vec![Layer::new("air", "Air")],
            entities: vec![entity],
        }
        .merge_into(&mut picture);
        picture
    }

    /// Merging A into B and B into A ends with the same picture, byte for byte,
    /// whether the tie on `last_seen` is broken by `last_received` or, on a full
    /// tie, by the canonical bytes. [VLD-ANB-MIDGARD-0015] [AC-23.5] [FR-H-6]
    #[test]
    fn equal_last_seen_merges_converge() {
        for (a, b) in [
            (
                entity("radar-1", 420, 200, 1_000),
                entity("radar-2", 430, 200, 1_005),
            ),
            (
                entity("radar-1", 420, 200, 1_000),
                entity("radar-2", 430, 200, 1_000),
            ),
        ] {
            let mut a_then_b = picture_of(a.clone());
            PictureDelta::snapshot(&picture_of(b.clone())).merge_into(&mut a_then_b);
            let mut b_then_a = picture_of(b);
            PictureDelta::snapshot(&picture_of(a)).merge_into(&mut b_then_a);
            assert_eq!(
                PictureDelta::snapshot(&a_then_b).canonical_bytes(),
                PictureDelta::snapshot(&b_then_a).canonical_bytes()
            );
        }
        // The later receipt is the one both keep.
        let mut picture = picture_of(entity("radar-2", 430, 200, 1_005));
        PictureDelta::snapshot(&picture_of(entity("radar-1", 420, 200, 1_000)))
            .merge_into(&mut picture);
        assert_eq!(
            picture
                .entity(&EntityId("AC-1".into()))
                .unwrap()
                .latest_attrs["speed"],
            Attr::Int(430)
        );
    }
}
