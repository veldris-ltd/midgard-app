//! Picture deltas — the unit of cross-node replication. A [`PictureDelta`] is a set
//! of upserted entities plus the layers they need, canonical-encoded so a peer node
//! can verify a post-quantum signature over it and merge it deterministically.
//! [FR-H-6]

use crate::codec;
use crate::entity::TrackedEntity;
use crate::error::PictureError;
use crate::layer::Layer;
use crate::picture::Picture;

/// A replicable slice of a picture: the entities to upsert and the layers they
/// belong to. Merge is **last-write-wins by `last_seen`**, which is commutative and
/// idempotent, so two nodes exchanging deltas in any order converge. [FR-H-6]
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
    /// incoming entity replaces the local one only if it is at least as recent.
    /// Returns how many entities were newly inserted or updated.
    pub fn merge_into(&self, picture: &mut Picture) -> usize {
        for layer in &self.layers {
            picture.ensure_layer(layer.clone());
        }
        let mut changed = 0;
        for incoming in &self.entities {
            match picture.entity(&incoming.id) {
                Some(local) if local.last_seen() > incoming.last_seen() => {
                    // Local state is strictly newer — keep it.
                }
                _ => {
                    picture.upsert_entity(incoming.clone());
                    changed += 1;
                }
            }
        }
        changed
    }
}
