//! [`DurablePicture`] — the common operating picture, KMS-sealed at rest and
//! recovered on boot.
//!
//! The picture is kept as a sealed snapshot plus a sealed journal of the changes
//! since it was taken:
//! - `picture.sealed` (the path given to [`DurablePicture::open`]) is the snapshot:
//!   the picture's own canonical [`PictureDelta`] snapshot (entities + layers) plus
//!   the local layer-visibility toggles, sealed as one blob and written atomically
//!   (temp + `fsync` + rename). It is the format earlier builds resealed the whole
//!   picture in on every change, so a picture they wrote opens as a snapshot with an
//!   empty journal.
//! - `picture.journal` beside it is an [`audit_writer::SealedJournal`] (domain
//!   `heimdall-picture-v1`): each change appends one sealed, `fsync`ed frame naming
//!   one entity or layer, so a change costs the size of what changed, not of the
//!   picture. [VLD-ANB-MIDGARD-0017]
//!
//! [`DurablePicture::compact`] folds the journal into a fresh snapshot and starts an
//! empty journal; it runs by itself once the journal holds more than twice as many
//! frames as the picture holds entities, plus 1,000. On [`DurablePicture::open`] the
//! snapshot is unsealed and the journal replayed over it (both fail closed on at-rest
//! tamper). The crypto lives here, not in the pure-std picture crate. [FR-H-1]
//! [NFR-PQ-2] [RC-13]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use audit_writer::{AuditError, SealedJournal, Sealer};
use heimdall_picture::Layer;
use heimdall_picture::{EntityId, LayerId, Picture, PictureDelta, TrackedEntity};

use crate::error::StoreError;
use crate::util::{atomic_write, put_str, Reader};

/// The AAD the snapshot is sealed under (unchanged from the whole-picture format).
const AAD: &[u8] = b"midgard-heimdall-picture-v1";
/// The domain the journal's frames are sealed under.
const JOURNAL_DOMAIN: &[u8] = b"heimdall-picture-v1";

/// Journal frame: an entity inserted or replaced (its canonical bytes follow).
const OP_UPSERT_ENTITY: u8 = 1;
/// Journal frame: an entity removed (its id follows).
const OP_REMOVE_ENTITY: u8 = 2;
/// Journal frame: a layer registered (its id and name follow).
const OP_ENSURE_LAYER: u8 = 3;
/// Journal frame: a layer shown or hidden (its id and a visibility byte follow).
const OP_SET_LAYER_VISIBLE: u8 = 4;

/// Frames the journal may hold beyond twice the entity count before it is compacted.
const COMPACTION_SLACK: u64 = 1_000;

/// A common operating picture whose state is durable and encrypted at rest.
pub struct DurablePicture {
    picture: Picture,
    path: PathBuf,
    journal: SealedJournal,
    sealer: Arc<dyn Sealer>,
}

impl DurablePicture {
    /// Open (creating if absent) the sealed picture at `path` and its journal beside
    /// it, unsealing the snapshot and replaying the journal over it. Fails closed on
    /// at-rest tamper or corruption.
    pub fn open(path: impl AsRef<Path>, sealer: Box<dyn Sealer>) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        let sealer: Arc<dyn Sealer> = Arc::from(sealer);
        let mut picture = if path.exists() {
            let sealed = std::fs::read(&path).map_err(crate::util::io)?;
            let plaintext = sealer
                .unseal(AAD, &sealed)
                .map_err(|e| StoreError::Seal(e.to_string()))?;
            decode_picture(&plaintext)?
        } else {
            Picture::new()
        };

        // A journal set aside by a compaction that stopped before deleting it: the
        // snapshot that replaced it was already durable, so it holds nothing new.
        let retired = retired_path(&path);
        if retired.exists() {
            std::fs::remove_file(&retired).map_err(crate::util::io)?;
        }
        let (journal, records) = SealedJournal::open(
            journal_path(&path),
            Box::new(SharedSealer(sealer.clone())),
            JOURNAL_DOMAIN,
        )
        .map_err(seal)?;
        for record in &records {
            replay(&mut picture, record)?;
        }

        let mut store = Self {
            picture,
            path,
            journal,
            sealer,
        };
        store.compact_if_due()?;
        Ok(store)
    }

    /// The recovered picture (read surface: entities, layers, visibility).
    pub fn picture(&self) -> &Picture {
        &self.picture
    }

    /// Insert or replace a tracked entity, durably: one journal frame.
    pub fn upsert_entity(&mut self, entity: TrackedEntity) -> Result<(), StoreError> {
        let delta = PictureDelta {
            layers: Vec::new(),
            entities: vec![entity],
        };
        let mut record = vec![OP_UPSERT_ENTITY];
        put_bytes(&mut record, &delta.canonical_bytes());
        self.append(&record)?;
        for entity in delta.entities {
            self.picture.upsert_entity(entity);
        }
        self.compact_if_due()
    }

    /// Remove a tracked entity, durably: one journal frame. Returns the entity if
    /// it was held; removing one that is not writes nothing.
    pub fn remove_entity(&mut self, id: &EntityId) -> Result<Option<TrackedEntity>, StoreError> {
        if self.picture.entity(id).is_none() {
            return Ok(None);
        }
        let mut record = vec![OP_REMOVE_ENTITY];
        put_str(&mut record, &id.0);
        self.append(&record)?;
        let removed = self.picture.remove_entity(id);
        self.compact_if_due()?;
        Ok(removed)
    }

    /// Register a layer if absent, durably. A layer already held writes nothing.
    pub fn ensure_layer(&mut self, layer: Layer) -> Result<(), StoreError> {
        if self.picture.layers().any(|l| l.id == layer.id) {
            return Ok(());
        }
        let mut record = vec![OP_ENSURE_LAYER];
        put_str(&mut record, &layer.id.0);
        put_str(&mut record, &layer.name);
        self.append(&record)?;
        self.picture.ensure_layer(layer);
        self.compact_if_due()
    }

    /// Toggle a layer's visibility, durably. No change writes nothing.
    pub fn set_layer_visible(&mut self, layer: &LayerId, visible: bool) -> Result<(), StoreError> {
        if self.picture.is_layer_visible(layer) == visible {
            return Ok(());
        }
        let mut record = vec![OP_SET_LAYER_VISIBLE];
        put_str(&mut record, &layer.0);
        record.push(u8::from(visible));
        self.append(&record)?;
        self.picture.set_layer_visible(layer, visible);
        self.compact_if_due()
    }

    /// Frames appended to the journal since the last compaction.
    pub fn frames_since_compaction(&self) -> u64 {
        self.journal.len()
    }

    /// The journal's path: `picture.journal` beside the snapshot.
    pub fn journal_path(&self) -> PathBuf {
        journal_path(&self.path)
    }

    /// Fold the journal into a fresh snapshot and start an empty journal. The
    /// snapshot is durable (atomic write) before the old journal is set aside, and
    /// the old journal is deleted only once the new one is open, so a crash at any
    /// point leaves a snapshot and journal that replay to the same picture.
    pub fn compact(&mut self) -> Result<(), StoreError> {
        let plaintext = encode_picture(&self.picture);
        let sealed = self
            .sealer
            .seal(AAD, &plaintext)
            .map_err(|e| StoreError::Seal(e.to_string()))?;
        atomic_write(&self.path, &sealed)?;

        let journal = journal_path(&self.path);
        let retired = retired_path(&self.path);
        std::fs::rename(&journal, &retired).map_err(crate::util::io)?;
        let (fresh, _) = SealedJournal::open(
            &journal,
            Box::new(SharedSealer(self.sealer.clone())),
            JOURNAL_DOMAIN,
        )
        .map_err(seal)?;
        self.journal = fresh;
        std::fs::remove_file(&retired).map_err(crate::util::io)?;
        Ok(())
    }

    fn append(&mut self, record: &[u8]) -> Result<(), StoreError> {
        self.journal.append(record).map_err(seal)
    }

    fn compact_if_due(&mut self) -> Result<(), StoreError> {
        let entities = self.picture.entity_count() as u64;
        if self.journal.len() > entities.saturating_mul(2) + COMPACTION_SLACK {
            self.compact()?;
        }
        Ok(())
    }
}

/// One sealer behind both the snapshot and the journal, which takes its own box.
struct SharedSealer(Arc<dyn Sealer>);

impl Sealer for SharedSealer {
    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
        self.0.seal(aad, plaintext)
    }
    fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
        self.0.unseal(aad, sealed)
    }
}

fn journal_path(snapshot: &Path) -> PathBuf {
    snapshot.with_extension("journal")
}

fn retired_path(snapshot: &Path) -> PathBuf {
    snapshot.with_extension("journal.old")
}

fn seal(e: AuditError) -> StoreError {
    StoreError::Seal(e.to_string())
}

fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

/// Apply one journal frame to `picture`. A frame states the value it leaves, so
/// replaying frames a snapshot already holds changes nothing.
fn replay(picture: &mut Picture, record: &[u8]) -> Result<(), StoreError> {
    let mut r = Reader::new(record);
    match r.u8()? {
        OP_UPSERT_ENTITY => {
            let delta = PictureDelta::from_canonical_bytes(r.bytes()?)
                .map_err(|e| StoreError::Decode(e.to_string()))?;
            if !delta.layers.is_empty() || delta.entities.len() != 1 {
                return Err(StoreError::Decode(
                    "a journal upsert names other than one entity".into(),
                ));
            }
            for entity in delta.entities {
                picture.upsert_entity(entity);
            }
        }
        OP_REMOVE_ENTITY => {
            picture.remove_entity(&EntityId(r.string()?));
        }
        OP_ENSURE_LAYER => {
            let id = r.string()?;
            let name = r.string()?;
            picture.ensure_layer(Layer::new(id, name));
        }
        OP_SET_LAYER_VISIBLE => {
            let id = LayerId(r.string()?);
            let visible = match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err(StoreError::Decode("journal visibility byte".into())),
            };
            picture.set_layer_visible(&id, visible);
        }
        op => return Err(StoreError::Decode(format!("unknown journal op {op}"))),
    }
    if !r.done() {
        return Err(StoreError::Decode("journal frame trailing bytes".into()));
    }
    Ok(())
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
