//! The canonical codec: deterministic, self-describing byte encodings of the
//! signable/replicable model types. Encode is total; decode fails closed on any
//! malformed input. Same value → same bytes → same signature, on any host, offline.
//! [FR-H-3] [FR-H-6] [NFR-PRV-1]

use std::collections::BTreeMap;

use crate::delta::PictureDelta;
use crate::entity::{Classification, ClassificationSource, Track, TrackedEntity};
use crate::error::PictureError;
use crate::id::{EntityId, LayerId};
use crate::layer::Layer;
use crate::observation::{Attr, Observation};

/// Format version — bump only on a breaking encoding change.
const FORMAT_VERSION: u8 = 0x01;

// Attribute value tags.
const TAG_TEXT: u8 = 1;
const TAG_INT: u8 = 2;
const TAG_FLOAT: u8 = 3;
const TAG_BOOL: u8 = 4;

// --- writer helpers -------------------------------------------------------

fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_u64(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

fn put_attr(out: &mut Vec<u8>, a: &Attr) {
    match a {
        Attr::Text(s) => {
            out.push(TAG_TEXT);
            put_str(out, s);
        }
        Attr::Int(i) => {
            out.push(TAG_INT);
            put_u64(out, *i as u64);
        }
        Attr::Float(f) => {
            out.push(TAG_FLOAT);
            put_u64(out, f.to_bits());
        }
        Attr::Bool(b) => {
            out.push(TAG_BOOL);
            out.push(*b as u8);
        }
    }
}

fn put_attrs(out: &mut Vec<u8>, attrs: &BTreeMap<String, Attr>) {
    put_u64(out, attrs.len() as u64);
    for (k, v) in attrs {
        put_str(out, k);
        put_attr(out, v);
    }
}

// --- encode ---------------------------------------------------------------

/// Encode an observation to its canonical signing bytes.
pub(crate) fn encode_observation(o: &Observation) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(FORMAT_VERSION);
    put_str(&mut out, &o.feed_id.0);
    put_str(&mut out, &o.id.0);
    put_str(&mut out, &o.kind);
    put_str(&mut out, &o.entity_key);
    put_u64(&mut out, o.observed_at);
    put_attrs(&mut out, &o.attrs);
    out
}

fn put_track(out: &mut Vec<u8>, t: &Track) {
    put_u64(out, t.first_seen);
    put_u64(out, t.last_seen);
    put_u64(out, t.observation_count);
}

fn put_classification(out: &mut Vec<u8>, c: &Option<Classification>) {
    match c {
        None => out.push(0),
        Some(c) => {
            out.push(1);
            put_str(out, &c.label);
            out.push(c.source.as_u8());
            put_str(out, &c.rationale);
        }
    }
}

fn put_entity(out: &mut Vec<u8>, e: &TrackedEntity) {
    put_str(out, &e.id.0);
    put_str(out, &e.entity_key);
    put_str(out, &e.kind);
    put_str(out, &e.layer.0);
    put_u64(out, e.sources.len() as u64);
    for s in &e.sources {
        put_str(out, s);
    }
    put_track(out, &e.track);
    put_classification(out, &e.classification);
    put_attrs(out, &e.latest_attrs);
}

/// Encode a picture delta to its canonical replication bytes.
pub(crate) fn encode_delta(d: &PictureDelta) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(FORMAT_VERSION);
    put_u64(&mut out, d.layers.len() as u64);
    for l in &d.layers {
        put_str(&mut out, &l.id.0);
        put_str(&mut out, &l.name);
    }
    put_u64(&mut out, d.entities.len() as u64);
    for e in &d.entities {
        put_entity(&mut out, e);
    }
    out
}

// --- decode ---------------------------------------------------------------

/// A bounds-checked reader over canonical bytes.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn u8(&mut self) -> Result<u8, PictureError> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| PictureError::Decode("unexpected end of buffer".into()))?;
        self.pos += 1;
        Ok(b)
    }

    fn u64(&mut self) -> Result<u64, PictureError> {
        let end = self
            .pos
            .checked_add(8)
            .ok_or_else(|| PictureError::Decode("length overflow".into()))?;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| PictureError::Decode("truncated u64".into()))?;
        let mut a = [0u8; 8];
        a.copy_from_slice(slice);
        self.pos = end;
        Ok(u64::from_le_bytes(a))
    }

    fn string(&mut self) -> Result<String, PictureError> {
        let n = self.u64()? as usize;
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| PictureError::Decode("length overflow".into()))?;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| PictureError::Decode("truncated string".into()))?;
        let s = std::str::from_utf8(slice)
            .map_err(|_| PictureError::Decode("invalid utf-8".into()))?
            .to_string();
        self.pos = end;
        Ok(s)
    }

    fn attr(&mut self) -> Result<Attr, PictureError> {
        match self.u8()? {
            TAG_TEXT => Ok(Attr::Text(self.string()?)),
            TAG_INT => Ok(Attr::Int(self.u64()? as i64)),
            TAG_FLOAT => Ok(Attr::Float(f64::from_bits(self.u64()?))),
            TAG_BOOL => Ok(Attr::Bool(self.u8()? != 0)),
            other => Err(PictureError::Decode(format!("unknown attr tag {other}"))),
        }
    }

    fn attrs(&mut self) -> Result<BTreeMap<String, Attr>, PictureError> {
        let n = self.u64()? as usize;
        let mut map = BTreeMap::new();
        for _ in 0..n {
            let k = self.string()?;
            let v = self.attr()?;
            map.insert(k, v);
        }
        Ok(map)
    }

    fn track(&mut self) -> Result<Track, PictureError> {
        Ok(Track {
            first_seen: self.u64()?,
            last_seen: self.u64()?,
            observation_count: self.u64()?,
        })
    }

    fn classification(&mut self) -> Result<Option<Classification>, PictureError> {
        match self.u8()? {
            0 => Ok(None),
            1 => {
                let label = self.string()?;
                let source = ClassificationSource::from_u8(self.u8()?)
                    .ok_or_else(|| PictureError::Decode("unknown classification source".into()))?;
                let rationale = self.string()?;
                Ok(Some(Classification {
                    label,
                    source,
                    rationale,
                }))
            }
            other => Err(PictureError::Decode(format!(
                "invalid classification tag {other}"
            ))),
        }
    }

    fn sources(&mut self) -> Result<std::collections::BTreeSet<String>, PictureError> {
        let n = self.u64()? as usize;
        let mut set = std::collections::BTreeSet::new();
        for _ in 0..n {
            set.insert(self.string()?);
        }
        Ok(set)
    }

    fn entity(&mut self) -> Result<TrackedEntity, PictureError> {
        let id = EntityId(self.string()?);
        let entity_key = self.string()?;
        let kind = self.string()?;
        let layer = LayerId(self.string()?);
        let sources = self.sources()?;
        let track = self.track()?;
        let classification = self.classification()?;
        let latest_attrs = self.attrs()?;
        Ok(TrackedEntity {
            id,
            entity_key,
            kind,
            layer,
            sources,
            track,
            classification,
            latest_attrs,
        })
    }
}

/// Decode canonical bytes into a picture delta. Fails closed on any malformed input.
pub(crate) fn decode_delta(buf: &[u8]) -> Result<PictureDelta, PictureError> {
    let mut r = Reader::new(buf);
    let version = r.u8()?;
    if version != FORMAT_VERSION {
        return Err(PictureError::Decode(format!(
            "unsupported delta format version {version}"
        )));
    }
    let layer_count = r.u64()? as usize;
    let mut layers = Vec::with_capacity(layer_count.min(1024));
    for _ in 0..layer_count {
        let id = r.string()?;
        let name = r.string()?;
        layers.push(Layer::new(id, name));
    }
    let entity_count = r.u64()? as usize;
    let mut entities = Vec::with_capacity(entity_count.min(1024));
    for _ in 0..entity_count {
        entities.push(r.entity()?);
    }
    if r.pos != buf.len() {
        return Err(PictureError::Decode("trailing bytes after delta".into()));
    }
    Ok(PictureDelta { layers, entities })
}

/// Decode canonical bytes into an observation. The exact inverse of
/// [`encode_observation`]; fails closed on any malformed input. Lets a receiver
/// reconstruct the observation a feed signed and transmitted. [FR-H-3]
pub(crate) fn decode_observation(buf: &[u8]) -> Result<Observation, PictureError> {
    use crate::id::{FeedId, ObservationId};

    let mut r = Reader::new(buf);
    let version = r.u8()?;
    if version != FORMAT_VERSION {
        return Err(PictureError::Decode(format!(
            "unsupported observation format version {version}"
        )));
    }
    let feed_id = FeedId(r.string()?);
    let id = ObservationId(r.string()?);
    let kind = r.string()?;
    let entity_key = r.string()?;
    let observed_at = r.u64()?;
    let attrs = r.attrs()?;
    if r.pos != buf.len() {
        return Err(PictureError::Decode(
            "trailing bytes after observation".into(),
        ));
    }
    Ok(Observation {
        id,
        feed_id,
        kind,
        entity_key,
        attrs,
        observed_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entity(id: &str, last_seen: u64) -> TrackedEntity {
        let mut attrs = BTreeMap::new();
        attrs.insert("speed".into(), Attr::Float(420.5));
        attrs.insert("hostile".into(), Attr::Bool(true));
        attrs.insert("count".into(), Attr::Int(-3));
        attrs.insert("callsign".into(), Attr::Text("VIPER1".into()));
        let mut sources = std::collections::BTreeSet::new();
        sources.insert("radar-1".to_string());
        sources.insert("sonar-2".to_string());
        TrackedEntity {
            id: EntityId(id.into()),
            entity_key: format!("key-{id}"),
            kind: "air-track".into(),
            layer: LayerId("air".into()),
            sources,
            track: Track {
                first_seen: 10,
                last_seen,
                observation_count: 4,
            },
            classification: Some(Classification {
                label: "fast-mover".into(),
                source: ClassificationSource::Rules,
                rationale: "speed > 400".into(),
            }),
            latest_attrs: attrs,
        }
    }

    #[test]
    fn delta_round_trips() {
        let delta = PictureDelta {
            layers: vec![Layer::new("air", "Air"), Layer::new("sea", "Sea")],
            entities: vec![sample_entity("e1", 20), sample_entity("e2", 25)],
        };
        let bytes = delta.canonical_bytes();
        let back = PictureDelta::from_canonical_bytes(&bytes).expect("decodes");
        assert_eq!(delta, back);
    }

    #[test]
    fn encoding_is_deterministic() {
        let a = sample_entity("e1", 20);
        let d1 = PictureDelta {
            layers: vec![],
            entities: vec![a.clone()],
        };
        let d2 = PictureDelta {
            layers: vec![],
            entities: vec![a],
        };
        assert_eq!(d1.canonical_bytes(), d2.canonical_bytes());
    }

    #[test]
    fn observation_bytes_are_stable() {
        let o = Observation::new(
            "o1",
            crate::id::FeedId("radar-1".into()),
            "air-track",
            "trk-9",
            42,
        )
        .with_attr("speed", Attr::Float(500.0));
        assert_eq!(o.canonical_bytes(), o.canonical_bytes());
    }

    #[test]
    fn observation_round_trips() {
        let o = Observation::new(
            "o1",
            crate::id::FeedId("radar-1".into()),
            "air-track",
            "trk-9",
            42,
        )
        .with_attr("speed", Attr::Float(500.0))
        .with_attr("callsign", Attr::Text("VIPER1".into()))
        .with_attr("hostile", Attr::Bool(true))
        .with_attr("count", Attr::Int(-7));
        let bytes = o.canonical_bytes();
        let back = Observation::from_canonical_bytes(&bytes).expect("decodes");
        assert_eq!(o, back);
        // A trailing byte fails closed.
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(Observation::from_canonical_bytes(&extra).is_err());
    }

    #[test]
    fn truncated_buffer_fails_closed() {
        let delta = PictureDelta {
            layers: vec![Layer::new("air", "Air")],
            entities: vec![sample_entity("e1", 20)],
        };
        let bytes = delta.canonical_bytes();
        assert!(PictureDelta::from_canonical_bytes(&bytes[..bytes.len() - 3]).is_err());
    }

    #[test]
    fn trailing_bytes_rejected() {
        let delta = PictureDelta {
            layers: vec![],
            entities: vec![],
        };
        let mut bytes = delta.canonical_bytes();
        bytes.push(0xFF);
        assert!(PictureDelta::from_canonical_bytes(&bytes).is_err());
    }
}
