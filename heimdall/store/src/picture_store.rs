//! [`DurablePicture`] — the common operating picture, KMS-sealed at rest and
//! recovered on boot.
//!
//! The whole picture is snapshotted as a single sealed blob on each change
//! (atomic temp + `fsync` + rename). The plaintext is the picture's own canonical
//! [`PictureDelta`] snapshot (entities + layers) plus the local layer-visibility
//! toggles, all via `heimdall-picture`'s public API — so the crypto lives here, not
//! in the pure-std picture crate. On [`DurablePicture::open`] the blob is unsealed
//! (fails closed on at-rest tamper) and merged back into a fresh picture. [FR-H-1]
//! [NFR-PQ-2] [RC-13]

use std::path::{Path, PathBuf};

use audit_writer::Sealer;
use heimdall_picture::Layer;
use heimdall_picture::{EntityId, LayerId, Picture, PictureDelta, TrackedEntity};

use crate::error::StoreError;
use crate::util::{atomic_write, put_str, Reader};

const AAD: &[u8] = b"midgard-heimdall-picture-v1";

/// A common operating picture whose state is durable and encrypted at rest.
pub struct DurablePicture {
    picture: Picture,
    path: PathBuf,
    sealer: Box<dyn Sealer>,
}

impl DurablePicture {
    /// Open (creating if absent) the sealed picture at `path`, unsealing and
    /// recovering it. Fails closed on at-rest tamper or corruption.
    pub fn open(path: impl AsRef<Path>, sealer: Box<dyn Sealer>) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        let picture = if path.exists() {
            let sealed = std::fs::read(&path).map_err(crate::util::io)?;
            let plaintext = sealer
                .unseal(AAD, &sealed)
                .map_err(|e| StoreError::Seal(e.to_string()))?;
            decode_picture(&plaintext)?
        } else {
            Picture::new()
        };
        Ok(Self {
            picture,
            path,
            sealer,
        })
    }

    /// The recovered picture (read surface: entities, layers, visibility).
    pub fn picture(&self) -> &Picture {
        &self.picture
    }

    /// Insert or replace a tracked entity, durably.
    pub fn upsert_entity(&mut self, entity: TrackedEntity) -> Result<(), StoreError> {
        self.picture.upsert_entity(entity);
        self.save()
    }

    /// Remove every entity last seen before `cutoff`, durably, in one write. Returns the
    /// removed ids; nothing is written when none expire. [FR-H-1] [RC-13]
    pub fn expire_before(&mut self, cutoff: u64) -> Result<Vec<EntityId>, StoreError> {
        let gone = self.picture.expire_before(cutoff);
        if !gone.is_empty() {
            self.save()?;
        }
        Ok(gone)
    }

    /// Register layers if absent and insert or replace entities, durably, in one write.
    /// [RC-13]
    pub fn upsert_all(
        &mut self,
        layers: impl IntoIterator<Item = Layer>,
        entities: impl IntoIterator<Item = TrackedEntity>,
    ) -> Result<(), StoreError> {
        for layer in layers {
            self.picture.ensure_layer(layer);
        }
        for entity in entities {
            self.picture.upsert_entity(entity);
        }
        self.save()
    }

    /// Register a layer if absent, durably.
    pub fn ensure_layer(&mut self, layer: Layer) -> Result<(), StoreError> {
        self.picture.ensure_layer(layer);
        self.save()
    }

    /// Toggle a layer's visibility, durably.
    pub fn set_layer_visible(&mut self, layer: &LayerId, visible: bool) -> Result<(), StoreError> {
        self.picture.set_layer_visible(layer, visible);
        self.save()
    }

    fn save(&mut self) -> Result<(), StoreError> {
        let plaintext = encode_picture(&self.picture);
        let sealed = self
            .sealer
            .seal(AAD, &plaintext)
            .map_err(|e| StoreError::Seal(e.to_string()))?;
        atomic_write(&self.path, &sealed)
    }
}

/// Canonical picture bytes: the full `PictureDelta` snapshot (entities + layers)
/// plus the hidden-layer set (local toggles the replication delta omits).
fn encode_picture(p: &Picture) -> Vec<u8> {
    let db = PictureDelta::snapshot(p).canonical_bytes();
    let hidden: Vec<&LayerId> = p
        .layers()
        .map(|l| &l.id)
        .filter(|id| !p.is_layer_visible(id))
        .collect();

    let mut o = Vec::with_capacity(4 + db.len() + 4);
    o.extend_from_slice(&(db.len() as u32).to_le_bytes());
    o.extend_from_slice(&db);
    o.extend_from_slice(&(hidden.len() as u32).to_le_bytes());
    for id in hidden {
        put_str(&mut o, &id.0);
    }
    o
}

fn decode_picture(b: &[u8]) -> Result<Picture, StoreError> {
    let mut r = Reader::new(b);
    let delta_bytes = r.bytes()?;
    let delta = PictureDelta::from_canonical_bytes(delta_bytes)
        .map_err(|e| StoreError::Decode(e.to_string()))?;
    let mut picture = Picture::new();
    delta.merge_into(&mut picture);

    let hidden = r.u32()?;
    for _ in 0..hidden {
        picture.set_layer_visible(&LayerId(r.string()?), false);
    }
    if !r.done() {
        return Err(StoreError::Decode("picture blob trailing bytes".into()));
    }
    Ok(picture)
}
