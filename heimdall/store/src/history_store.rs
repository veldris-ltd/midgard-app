//! [`DurableHistory`] — where each tracked entity has been, sealed at rest, recovered
//! on boot, and kept for a retention window. [FR-H-1] [NFR-PQ-2] [RC-13]
//!
//! The live picture is current state; this is its recent past: one point
//! `(t, lat, lon, alt_m)` per positioned observation fused, grouped by entity, with
//! the entity's kind and layer.
//!
//! ## At rest
//!
//! A directory of numbered *segments* (`<id>.seg`). A segment is an append-only run of
//! frames — a little-endian `u32` length, then the bytes the [`Sealer`] produced —
//! each sealing one batch of points under an AAD that binds the segment id and the
//! frame's position, so a frame cannot be reordered, moved to another segment or
//! altered without failing authentication. Appending a batch writes one frame and
//! `fsync`s it: nothing already written is resealed.
//!
//! One segment is open for appends at a time. It is closed after
//! [`SEGMENT_SECS`] or [`SEGMENT_BYTES`], and on every restart. Maintenance
//! ([`DurableHistory::maintain`]) then deletes closed segments that hold nothing newer
//! than the retention cutoff, and compacts a closed segment that has several frames or
//! holds points older than the cutoff: its points are merged per entity, the expired
//! ones dropped, and the result written as a few large frames to `<id>.cseg` (temp
//! file, `fsync`, rename; then the `.seg` it replaces is removed). So each point is resealed about once, in bulk, and the per-frame sealing
//! overhead of small batches does not stay on disk.
//!
//! In a frame, each track is its entity, kind and layer (length-prefixed UTF-8) and its
//! points; a point is `t` (`u64`) and latitude, longitude (degrees × 10⁵) and altitude
//! (whole metres, `i32::MIN` for none) as `i32` — 20 bytes, at the precision history is
//! served at.
//!
//! A frame cut short by a crash mid-append (a torn tail) is dropped and the file
//! truncated to its last whole frame; a whole frame that does not authenticate fails
//! the open closed, like the picture.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use audit_writer::Sealer;
use heimdall_picture::Attr;

use crate::error::StoreError;
use crate::util::{atomic_write, io, put_str, Reader};

const AAD_DS: &[u8] = b"midgard-heimdall-history-v1";
/// The longest an open segment takes appends before it is closed, in seconds.
pub const SEGMENT_SECS: u64 = 3600;
/// The most bytes an open segment takes before it is closed.
pub const SEGMENT_BYTES: u64 = 64 * 1024 * 1024;
/// The most plaintext one compacted frame holds.
const COMPACT_FRAME_BYTES: usize = 4 * 1024 * 1024;
/// No altitude.
const NO_ALT: i32 = i32::MIN;
/// What one point costs in a frame.
const POINT_BYTES: usize = 20;

/// One positioned observation to record.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRecord {
    /// The entity id, as the picture reports it.
    pub entity: String,
    /// The entity's kind.
    pub kind: String,
    /// The entity's layer id.
    pub layer: String,
    /// When it was observed (unix seconds).
    pub t: u64,
    /// Latitude, degrees.
    pub lat: f64,
    /// Longitude, degrees.
    pub lon: f64,
    /// Altitude, metres, if known.
    pub alt_m: Option<f64>,
}

/// The position an observation's attributes carry: numeric `lat` and `lon`, and an
/// altitude in metres from `alt_m`, else `alt_ft` × 0.3048, else `alt_km` × 1000.
/// `None` without a numeric, in-range latitude and longitude. [FR-H-1]
pub fn position_of(attrs: &BTreeMap<String, Attr>) -> Option<(f64, f64, Option<f64>)> {
    let num = |key: &str| match attrs.get(key) {
        Some(Attr::Float(f)) if f.is_finite() => Some(*f),
        Some(Attr::Int(i)) => Some(*i as f64),
        _ => None,
    };
    let lat = num("lat").filter(|v| (-90.0..=90.0).contains(v))?;
    let lon = num("lon").filter(|v| (-180.0..=180.0).contains(v))?;
    let alt = num("alt_m")
        .or_else(|| num("alt_ft").map(|ft| ft * 0.3048))
        .or_else(|| num("alt_km").map(|km| km * 1000.0));
    Some((lat, lon, alt))
}

/// A stored point, at served precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Point {
    t: u64,
    lat: i32,
    lon: i32,
    alt: i32,
}

impl Point {
    fn from_record(r: &HistoryRecord) -> Self {
        let alt = r
            .alt_m
            .filter(|a| a.is_finite())
            .map(|a| a.round().clamp(-1.0e9, 1.0e9) as i32)
            .unwrap_or(NO_ALT);
        Self {
            t: r.t,
            lat: (r.lat * 1e5).round() as i32,
            lon: (r.lon * 1e5).round() as i32,
            alt,
        }
    }

    fn alt_m(&self) -> Option<i32> {
        (self.alt != NO_ALT).then_some(self.alt)
    }

    /// `[t,lat,lon,alt]` as JSON.
    fn json(&self, out: &mut String) {
        use std::fmt::Write as _;
        let _ = write!(out, "[{},", self.t);
        fixed5(out, self.lat);
        out.push(',');
        fixed5(out, self.lon);
        match self.alt_m() {
            Some(a) => {
                let _ = write!(out, ",{a}]");
            }
            None => out.push_str(",null]"),
        }
    }
}

/// `v / 10⁵` in decimal, exactly, without trailing zeros.
fn fixed5(out: &mut String, v: i32) {
    use std::fmt::Write as _;
    let neg = v < 0;
    let a = i64::from(v).unsigned_abs();
    let (whole, frac) = (a / 100_000, a % 100_000);
    if neg {
        out.push('-');
    }
    let _ = write!(out, "{whole}");
    if frac != 0 {
        let digits = format!("{frac:05}");
        out.push('.');
        out.push_str(digits.trim_end_matches('0'));
    }
}

/// A decoded frame: each track's entity, kind, layer and points.
type Frame = Vec<(String, String, String, Vec<Point>)>;

/// One entity's history.
#[derive(Debug, Clone, Default)]
struct Track {
    kind: String,
    layer: String,
    /// Sorted by `t`, one point per second at most (a later report for the same
    /// second replaces the earlier).
    points: Vec<Point>,
}

impl Track {
    fn insert(&mut self, p: Point) {
        match self.points.last() {
            Some(last) if last.t < p.t => self.points.push(p),
            None => self.points.push(p),
            _ => match self.points.binary_search_by_key(&p.t, |q| q.t) {
                Ok(i) => self.points[i] = p,
                Err(i) => self.points.insert(i, p),
            },
        }
    }
}

/// What the store knows about a segment on disk.
#[derive(Debug, Clone)]
struct Segment {
    id: u64,
    /// Rewritten by compaction (`<id>.cseg`), rather than appended to (`<id>.seg`).
    compacted: bool,
    frames: u64,
    bytes: u64,
    min_t: u64,
    max_t: u64,
}

impl Segment {
    fn new(id: u64) -> Self {
        Self {
            id,
            compacted: false,
            frames: 0,
            bytes: 0,
            min_t: u64::MAX,
            max_t: 0,
        }
    }
    fn widen(&mut self, t: u64) {
        self.min_t = self.min_t.min(t);
        self.max_t = self.max_t.max(t);
    }
}

/// The segment taking appends.
struct Active {
    segment: Segment,
    file: File,
    opened_at: u64,
}

/// What a [`DurableHistory::maintain`] pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Maintenance {
    /// Points dropped from memory for being older than the cutoff.
    pub points_pruned: usize,
    /// Segments deleted.
    pub segments_deleted: usize,
    /// Segments rewritten.
    pub segments_compacted: usize,
}

/// A history read: the window, filters, decimation, where to resume and how big a
/// page may be.
#[derive(Debug, Clone)]
pub struct HistoryQuery {
    /// Earliest point, inclusive (unix seconds).
    pub from: u64,
    /// Latest point, inclusive.
    pub to: u64,
    /// Only this layer.
    pub layer: Option<String>,
    /// Only this entity.
    pub entity: Option<String>,
    /// The least spacing between returned points of one track, seconds (0: all).
    pub step: u64,
    /// Resume after this cursor (from a previous page's `next`).
    pub after: Option<String>,
    /// The most bytes of JSON the page's tracks may take (at least one point is
    /// always returned when any qualifies).
    pub max_bytes: usize,
}

/// One track of a page.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackPage {
    /// The entity id.
    pub id: String,
    /// Its kind.
    pub kind: String,
    /// Its layer.
    pub layer: String,
    /// `(t, lat, lon, alt_m)`, sorted by `t`, lat/lon to 5 decimals, alt to 1 m.
    pub points: Vec<(u64, f64, f64, Option<i32>)>,
    json_points: Vec<String>,
}

/// A page of history.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryPage {
    /// The tracks with points in the window, by entity id.
    pub tracks: Vec<TrackPage>,
    /// The cursor to pass as [`HistoryQuery::after`] for the rest, if any remains.
    pub next: Option<String>,
}

impl HistoryPage {
    /// The page as JSON: `{"from","to","tracks":[{"id","kind","layer","points"}],
    /// "next"}`, each point `[t,lat,lon,alt_m|null]`.
    pub fn to_json(&self, from: u64, to: u64) -> String {
        let mut out = format!("{{\"from\":{from},\"to\":{to},\"tracks\":[");
        for (i, track) in self.tracks.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            track_head(&mut out, &track.id, &track.kind, &track.layer);
            out.push_str(&track.json_points.join(","));
            out.push_str("]}");
        }
        out.push_str("],\"next\":");
        match &self.next {
            Some(next) => json_str(&mut out, next),
            None => out.push_str("null"),
        }
        out.push('}');
        out
    }
}

fn track_head(out: &mut String, id: &str, kind: &str, layer: &str) {
    out.push_str("{\"id\":");
    json_str(out, id);
    out.push_str(",\"kind\":");
    json_str(out, kind);
    out.push_str(",\"layer\":");
    json_str(out, layer);
    out.push_str(",\"points\":[");
}

fn json_str(out: &mut String, s: &str) {
    use std::fmt::Write as _;
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A cursor: the last point a page returned, `"<t>:<entity>"`.
fn parse_cursor(cursor: &str) -> Result<(u64, String), StoreError> {
    let (t, entity) = cursor
        .split_once(':')
        .ok_or_else(|| StoreError::Decode("the cursor is not t:entity".into()))?;
    let t = t
        .parse()
        .map_err(|_| StoreError::Decode("the cursor's time is not a number".into()))?;
    Ok((t, entity.to_string()))
}

/// Track history, durable and sealed at rest.
pub struct DurableHistory {
    dir: PathBuf,
    sealer: Box<dyn Sealer>,
    tracks: BTreeMap<String, Track>,
    closed: Vec<Segment>,
    active: Option<Active>,
    next_id: u64,
    points: usize,
}

impl DurableHistory {
    /// Open (creating if absent) the history in `dir`, unsealing every segment.
    /// Recovered points are not pruned until the first [`DurableHistory::maintain`].
    pub fn open(dir: impl AsRef<Path>, sealer: Box<dyn Sealer>) -> Result<Self, StoreError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir).map_err(io)?;
        // id -> compacted?
        let mut found: BTreeMap<u64, bool> = BTreeMap::new();
        for entry in fs::read_dir(&dir).map_err(io)? {
            let path = entry.map_err(io)?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if let Some(stem) = name.strip_suffix(".cseg") {
                if let Ok(id) = stem.parse::<u64>() {
                    found.insert(id, true);
                }
            } else if let Some(stem) = name.strip_suffix(".seg") {
                if let Ok(id) = stem.parse::<u64>() {
                    found.entry(id).or_insert(false);
                }
            } else if name.ends_with(".tmp") {
                // An interrupted compaction; its segment is intact.
                fs::remove_file(&path).map_err(io)?;
            }
        }
        for (id, compacted) in &found {
            let raw = dir.join(format!("{id:016}.seg"));
            if *compacted && raw.exists() {
                // Compaction finished but did not get to remove what it replaced.
                fs::remove_file(&raw).map_err(io)?;
            }
        }
        let mut history = Self {
            dir,
            sealer,
            tracks: BTreeMap::new(),
            closed: Vec::new(),
            active: None,
            next_id: found.keys().next_back().map_or(1, |id| id + 1),
            points: 0,
        };
        for (id, compacted) in found {
            let mut segment = Segment::new(id);
            segment.compacted = compacted;
            for batch in history.load_segment(&segment, true)? {
                segment.frames += 1;
                for (entity, kind, layer, points) in batch {
                    let track = history.tracks.entry(entity).or_default();
                    track.kind = kind;
                    track.layer = layer;
                    for p in points {
                        segment.widen(p.t);
                        track.insert(p);
                    }
                }
            }
            segment.bytes = fs::metadata(history.segment_path(&segment))
                .map_err(io)?
                .len();
            history.closed.push(segment);
        }
        history.recount();
        Ok(history)
    }

    /// How many points are held.
    pub fn point_count(&self) -> usize {
        self.points
    }

    /// How many entities have history.
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// How many segments are on disk.
    pub fn segment_count(&self) -> usize {
        self.closed.len() + usize::from(self.active.is_some())
    }

    /// Record a batch of points durably, in one sealed frame. `now` is the service
    /// clock (it ages the open segment). Nothing is written for an empty batch.
    pub fn record(&mut self, records: &[HistoryRecord], now: u64) -> Result<(), StoreError> {
        if records.is_empty() {
            return Ok(());
        }
        let mut grouped: BTreeMap<&str, (&str, &str, Vec<Point>)> = BTreeMap::new();
        for r in records {
            let entry = grouped.entry(r.entity.as_str()).or_insert((
                r.kind.as_str(),
                r.layer.as_str(),
                Vec::new(),
            ));
            entry.0 = r.kind.as_str();
            entry.1 = r.layer.as_str();
            entry.2.push(Point::from_record(r));
        }
        let plaintext = encode_frame(
            grouped
                .iter()
                .map(|(e, (k, l, p))| (*e, *k, *l, p.as_slice())),
        );

        if self
            .active
            .as_ref()
            .is_some_and(|a| a.segment.bytes >= SEGMENT_BYTES)
        {
            self.close_active();
        }
        if self.active.is_none() {
            self.open_active(now)?;
        }
        let Some(active) = self.active.as_mut() else {
            return Err(StoreError::Io("no open history segment".into()));
        };
        let sealed = self
            .sealer
            .seal(
                &frame_aad(active.segment.id, active.segment.frames),
                &plaintext,
            )
            .map_err(|e| StoreError::Seal(e.to_string()))?;
        let mut frame = Vec::with_capacity(4 + sealed.len());
        frame.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
        frame.extend_from_slice(&sealed);
        active.file.write_all(&frame).map_err(io)?;
        active.file.sync_data().map_err(io)?;
        active.segment.frames += 1;
        active.segment.bytes += frame.len() as u64;

        for (entity, (kind, layer, points)) in grouped {
            let track = self.tracks.entry(entity.to_string()).or_default();
            kind.clone_into(&mut track.kind);
            layer.clone_into(&mut track.layer);
            for p in points {
                active.segment.widen(p.t);
                let before = track.points.len();
                track.insert(p);
                self.points += track.points.len() - before;
            }
        }
        Ok(())
    }

    /// Drop points older than `cutoff` from memory, close the open segment once it is
    /// [`SEGMENT_SECS`] old, delete closed segments with nothing at or after `cutoff`,
    /// and compact closed segments with several frames or expired points.
    pub fn maintain(&mut self, now: u64, cutoff: u64) -> Result<Maintenance, StoreError> {
        let mut report = Maintenance::default();
        self.tracks.retain(|_, track| {
            let keep_from = track.points.partition_point(|p| p.t < cutoff);
            report.points_pruned += keep_from;
            track.points.drain(..keep_from);
            !track.points.is_empty()
        });
        self.points -= report.points_pruned;

        if self
            .active
            .as_ref()
            .is_some_and(|a| now.saturating_sub(a.opened_at) >= SEGMENT_SECS)
        {
            self.close_active();
        }

        let mut kept = Vec::with_capacity(self.closed.len());
        for segment in std::mem::take(&mut self.closed) {
            if segment.frames == 0 || segment.max_t < cutoff {
                fs::remove_file(self.segment_path(&segment)).map_err(io)?;
                report.segments_deleted += 1;
            } else {
                kept.push(segment);
            }
        }
        self.closed = kept;

        for i in 0..self.closed.len() {
            let segment = &self.closed[i];
            if (!segment.compacted && segment.frames > 1) || segment.min_t < cutoff {
                let compacted = self.compact(segment, cutoff)?;
                self.closed[i] = compacted;
                report.segments_compacted += 1;
            }
        }
        Ok(report)
    }

    /// A page of history, per [`HistoryQuery`]: tracks by entity id, points by time,
    /// decimated to `step`, resuming after the cursor, filling at most `max_bytes`.
    pub fn page(&self, q: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        let after = q.after.as_deref().map(parse_cursor).transpose()?;
        let start = match (&after, &q.entity) {
            (_, Some(entity)) => entity.clone(),
            (Some((_, entity)), None) => entity.clone(),
            (None, None) => String::new(),
        };
        let mut used = 0usize;
        let mut tracks = Vec::new();
        let mut next = None;
        'tracks: for (id, track) in self.tracks.range(start..) {
            if q.entity.as_ref().is_some_and(|e| e != id) {
                break;
            }
            if q.layer.as_ref().is_some_and(|l| *l != track.layer) {
                continue;
            }
            let mut last: Option<u64> = None;
            let mut lo = q.from;
            if let Some((t, entity)) = &after {
                match entity.as_str().cmp(id.as_str()) {
                    std::cmp::Ordering::Equal => {
                        last = Some(*t);
                        lo = lo.max(t.saturating_add(1));
                    }
                    std::cmp::Ordering::Greater => continue,
                    std::cmp::Ordering::Less => {}
                }
            }
            let begin = track.points.partition_point(|p| p.t < lo);
            let mut page = TrackPage {
                id: id.clone(),
                kind: track.kind.clone(),
                layer: track.layer.clone(),
                points: Vec::new(),
                json_points: Vec::new(),
            };
            let mut head = String::new();
            track_head(&mut head, id, &track.kind, &track.layer);
            for p in &track.points[begin..] {
                if p.t > q.to {
                    break;
                }
                if last.is_some_and(|l| p.t < l.saturating_add(q.step)) {
                    continue;
                }
                let mut json = String::new();
                p.json(&mut json);
                let cost = json.len()
                    + 1
                    + if page.points.is_empty() {
                        head.len() + 3
                    } else {
                        0
                    };
                if used + cost > q.max_bytes && (used > 0 || !page.points.is_empty()) {
                    let resume = page.points.last().map(|p| (p.0, id.clone())).or_else(|| {
                        tracks
                            .last()
                            .and_then(|t: &TrackPage| t.points.last().map(|p| (p.0, t.id.clone())))
                    });
                    next = resume.map(|(t, e)| format!("{t}:{e}"));
                    if !page.points.is_empty() {
                        tracks.push(page);
                    }
                    break 'tracks;
                }
                used += cost;
                last = Some(p.t);
                page.points.push((
                    p.t,
                    f64::from(p.lat) / 1e5,
                    f64::from(p.lon) / 1e5,
                    p.alt_m(),
                ));
                page.json_points.push(json);
            }
            if !page.points.is_empty() {
                tracks.push(page);
            }
        }
        Ok(HistoryPage { tracks, next })
    }

    fn segment_path(&self, segment: &Segment) -> PathBuf {
        let ext = if segment.compacted { "cseg" } else { "seg" };
        self.dir.join(format!("{:016}.{ext}", segment.id))
    }

    fn open_active(&mut self, now: u64) -> Result<(), StoreError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        // On Windows the file inherits its folder's ACL: the app's working copy, %LOCALAPPDATA%\Midgard, is private to the user (the W2 shell checks it).
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let segment = Segment::new(id);
        let file = opts.open(self.segment_path(&segment)).map_err(io)?;
        self.active = Some(Active {
            segment,
            file,
            opened_at: now,
        });
        Ok(())
    }

    fn close_active(&mut self) {
        if let Some(active) = self.active.take() {
            self.closed.push(active.segment);
        }
    }

    /// Unseal a segment's frames. With `repair`, a torn tail is truncated away.
    fn load_segment(&self, segment: &Segment, repair: bool) -> Result<Vec<Frame>, StoreError> {
        let id = segment.id;
        let path = self.segment_path(segment);
        let buf = fs::read(&path).map_err(io)?;
        let mut batches = Vec::new();
        let mut pos = 0usize;
        while pos < buf.len() {
            let Some(len) = buf
                .get(pos..pos + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            else {
                break;
            };
            let Some(sealed) = buf.get(pos + 4..pos + 4 + len) else {
                break;
            };
            let plaintext = self
                .sealer
                .unseal(&frame_aad(id, batches.len() as u64), sealed)
                .map_err(|e| StoreError::Seal(format!("history segment {id}: {e}")))?;
            batches.push(decode_frame(&plaintext)?);
            pos += 4 + len;
        }
        if pos < buf.len() {
            if !repair {
                return Err(StoreError::Decode(format!(
                    "history segment {id} is truncated"
                )));
            }
            OpenOptions::new()
                .write(true)
                .open(&path)
                .and_then(|f| f.set_len(pos as u64).and_then(|_| f.sync_all()))
                .map_err(io)?;
        }
        Ok(batches)
    }

    /// Rewrite a closed segment as merged, pruned, large frames.
    fn compact(&self, old: &Segment, cutoff: u64) -> Result<Segment, StoreError> {
        let id = old.id;
        let mut merged: BTreeMap<String, Track> = BTreeMap::new();
        for batch in self.load_segment(old, false)? {
            for (entity, kind, layer, points) in batch {
                let track = merged.entry(entity).or_default();
                track.kind = kind;
                track.layer = layer;
                for p in points.into_iter().filter(|p| p.t >= cutoff) {
                    track.insert(p);
                }
            }
        }
        merged.retain(|_, t| !t.points.is_empty());

        let mut segment = Segment::new(id);
        segment.compacted = true;
        let mut file = Vec::new();
        let mut chunk: Vec<(&str, &str, &str, &[Point])> = Vec::new();
        let mut chunk_bytes = 0usize;
        let mut flush = |chunk: &mut Vec<(&str, &str, &str, &[Point])>,
                         segment: &mut Segment|
         -> Result<(), StoreError> {
            if chunk.is_empty() {
                return Ok(());
            }
            let sealed = self
                .sealer
                .seal(
                    &frame_aad(id, segment.frames),
                    &encode_frame(chunk.iter().copied()),
                )
                .map_err(|e| StoreError::Seal(e.to_string()))?;
            file.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
            file.extend_from_slice(&sealed);
            segment.frames += 1;
            chunk.clear();
            Ok(())
        };
        for (entity, track) in &merged {
            for p in &track.points {
                segment.widen(p.t);
            }
            let per = (COMPACT_FRAME_BYTES / POINT_BYTES).max(1);
            for points in track.points.chunks(per) {
                let cost = 12
                    + entity.len()
                    + track.kind.len()
                    + track.layer.len()
                    + 4
                    + points.len() * POINT_BYTES;
                if chunk_bytes + cost > COMPACT_FRAME_BYTES {
                    flush(&mut chunk, &mut segment)?;
                    chunk_bytes = 0;
                }
                chunk.push((entity, &track.kind, &track.layer, points));
                chunk_bytes += cost;
            }
        }
        flush(&mut chunk, &mut segment)?;
        segment.bytes = file.len() as u64;
        atomic_write(&self.segment_path(&segment), &file)?;
        if !old.compacted {
            fs::remove_file(self.segment_path(old)).map_err(io)?;
        }
        Ok(segment)
    }

    fn recount(&mut self) {
        self.points = self.tracks.values().map(|t| t.points.len()).sum();
    }
}

fn frame_aad(segment: u64, frame: u64) -> Vec<u8> {
    let mut a = Vec::with_capacity(AAD_DS.len() + 16);
    a.extend_from_slice(AAD_DS);
    a.extend_from_slice(&segment.to_le_bytes());
    a.extend_from_slice(&frame.to_le_bytes());
    a
}

fn encode_frame<'a>(
    tracks: impl Iterator<Item = (&'a str, &'a str, &'a str, &'a [Point])>,
) -> Vec<u8> {
    let mut body = Vec::new();
    let mut n = 0u32;
    for (entity, kind, layer, points) in tracks {
        n += 1;
        put_str(&mut body, entity);
        put_str(&mut body, kind);
        put_str(&mut body, layer);
        body.extend_from_slice(&(points.len() as u32).to_le_bytes());
        for p in points {
            body.extend_from_slice(&p.t.to_le_bytes());
            body.extend_from_slice(&p.lat.to_le_bytes());
            body.extend_from_slice(&p.lon.to_le_bytes());
            body.extend_from_slice(&p.alt.to_le_bytes());
        }
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn decode_frame(b: &[u8]) -> Result<Frame, StoreError> {
    let mut r = Reader::new(b);
    let n = r.u32()?;
    let mut tracks = Vec::new();
    for _ in 0..n {
        let entity = r.string()?;
        let kind = r.string()?;
        let layer = r.string()?;
        let count = r.u32()?;
        let mut points = Vec::with_capacity(count.min(b.len() / POINT_BYTES));
        for _ in 0..count {
            let t = r.u64()?;
            let lat = r.i32()?;
            let lon = r.i32()?;
            let alt = r.i32()?;
            points.push(Point { t, lat, lon, alt });
        }
        tracks.push((entity, kind, layer, points));
    }
    if !r.done() {
        return Err(StoreError::Decode("history frame trailing bytes".into()));
    }
    Ok(tracks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use audit_writer::AuditError;

    /// A stand-in sealer for unit tests: it binds the AAD and a checksum, so a moved,
    /// reordered or altered frame fails to unseal. (The KMS sealer is exercised in the
    /// crate's integration tests.)
    struct TestSealer;
    fn sum(aad: &[u8], p: &[u8]) -> u64 {
        aad.iter().chain(p).fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
        })
    }
    impl Sealer for TestSealer {
        fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
            let mut out = sum(aad, plaintext).to_le_bytes().to_vec();
            out.extend(plaintext.iter().map(|b| b ^ 0x5a));
            Ok(out)
        }
        fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
            if sealed.len() < 8 {
                return Err(AuditError::Store("short".into()));
            }
            let plain: Vec<u8> = sealed[8..].iter().map(|b| b ^ 0x5a).collect();
            if sealed[..8] != sum(aad, &plain).to_le_bytes() {
                return Err(AuditError::Store("tampered".into()));
            }
            Ok(plain)
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let uniq = Box::into_raw(Box::new(0u8)) as usize;
        let dir = std::env::temp_dir().join(format!(
            "heimdall-history-{tag}-{}-{uniq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn open(dir: &Path) -> DurableHistory {
        DurableHistory::open(dir, Box::new(TestSealer)).unwrap()
    }

    fn rec(entity: &str, t: u64, lat: f64, lon: f64, alt: Option<f64>) -> HistoryRecord {
        HistoryRecord {
            entity: entity.into(),
            kind: "air-track".into(),
            layer: "air".into(),
            t,
            lat,
            lon,
            alt_m: alt,
        }
    }

    fn query(from: u64, to: u64) -> HistoryQuery {
        HistoryQuery {
            from,
            to,
            layer: None,
            entity: None,
            step: 0,
            after: None,
            max_bytes: usize::MAX,
        }
    }

    fn times(page: &HistoryPage, id: &str) -> Vec<u64> {
        page.tracks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.points.iter().map(|p| p.0).collect())
            .unwrap_or_default()
    }

    #[test]
    fn altitude_comes_from_metres_then_feet_then_kilometres() {
        let attrs = |pairs: &[(&str, Attr)]| -> BTreeMap<String, Attr> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect()
        };
        let base = [("lat", Attr::Float(51.5)), ("lon", Attr::Int(-1))];
        assert_eq!(position_of(&attrs(&base)), Some((51.5, -1.0, None)));
        let mut a = attrs(&base);
        a.insert("alt_ft".into(), Attr::Int(10_000));
        a.insert("alt_km".into(), Attr::Float(9.0));
        assert_eq!(position_of(&a).unwrap().2, Some(3048.0));
        a.insert("alt_m".into(), Attr::Float(120.5));
        assert_eq!(position_of(&a).unwrap().2, Some(120.5));
        let mut k = attrs(&base);
        k.insert("alt_km".into(), Attr::Float(1.5));
        assert_eq!(position_of(&k).unwrap().2, Some(1500.0));
        let mut text = attrs(&base);
        text.insert("lat".into(), Attr::Text("51.5".into()));
        assert_eq!(position_of(&text), None, "text is not a position");
        let mut far = attrs(&base);
        far.insert("lat".into(), Attr::Float(91.0));
        assert_eq!(position_of(&far), None, "out of range");
        assert_eq!(position_of(&attrs(&base[..1])), None, "no longitude");
    }

    #[test]
    fn points_are_recorded_sorted_rounded_and_recovered_after_reopen() {
        let dir = scratch("reopen");
        let mut h = open(&dir);
        h.record(
            &[
                rec("B", 20, 1.0, 2.0, None),
                rec("A", 30, 51.123456, -0.987654, Some(10_000.4)),
            ],
            1000,
        )
        .unwrap();
        h.record(
            &[
                rec("A", 10, -33.9, 151.2, Some(5.6)),
                rec("A", 30, 51.1, -0.9, None),
            ],
            1001,
        )
        .unwrap();
        assert_eq!(
            h.point_count(),
            3,
            "a second report for A at t=30 replaced the first"
        );
        drop(h);

        let h = open(&dir);
        assert_eq!(h.point_count(), 3);
        assert_eq!(h.track_count(), 2);
        let page = h.page(&query(0, 100)).unwrap();
        assert_eq!(page.next, None);
        assert_eq!(page.tracks[0].id, "A");
        assert_eq!(
            page.tracks[0].points,
            [(10, -33.9, 151.2, Some(6)), (30, 51.1, -0.9, None)]
        );
        assert_eq!(times(&page, "B"), [20]);
        let json = page.to_json(0, 100);
        assert!(
            json.contains(r#"{"id":"A","kind":"air-track","layer":"air","points":[[10,-33.9,151.2,6],[30,51.1,-0.9,null]]}"#),
            "{json}"
        );
        assert!(json.ends_with(r#""next":null}"#), "{json}");

        let mut fresh = open(&scratch("round"));
        fresh
            .record(&[rec("R", 1, 51.123456, -0.000004, Some(10_000.4))], 0)
            .unwrap();
        let p = fresh.page(&query(0, 9)).unwrap();
        assert_eq!(p.tracks[0].points, [(1, 51.12346, -0.0, Some(10_000))]);
        assert!(p.to_json(0, 9).contains("[1,51.12346,0,10000]"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_torn_tail_is_dropped_and_a_tampered_frame_fails_closed() {
        let dir = scratch("torn");
        let mut h = open(&dir);
        h.record(&[rec("A", 1, 1.0, 1.0, None)], 0).unwrap();
        h.record(&[rec("A", 2, 1.0, 1.0, None)], 0).unwrap();
        drop(h);
        let seg = dir.join(format!("{:016}.seg", 1));
        assert!(seg.exists());
        let mut bytes = fs::read(&seg).unwrap();
        bytes.truncate(bytes.len() - 3);
        fs::write(&seg, &bytes).unwrap();
        let h = open(&dir);
        assert_eq!(h.point_count(), 1, "the torn second frame is gone");
        drop(h);

        let mut bytes = fs::read(&seg).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&seg, &bytes).unwrap();
        assert!(DurableHistory::open(&dir, Box::new(TestSealer)).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn retention_prunes_memory_deletes_old_segments_and_compacts() {
        let dir = scratch("prune");
        let mut h = open(&dir);
        h.record(&[rec("OLD", 100, 1.0, 1.0, None)], 100).unwrap();
        h.record(&[rec("OLD", 150, 1.0, 1.0, None)], 150).unwrap();
        // The segment is an hour old by now: closed, then compacted.
        let report = h.maintain(100 + SEGMENT_SECS, 0).unwrap();
        assert_eq!(report.segments_compacted, 1);
        assert_eq!(report.points_pruned, 0);
        h.record(
            &[
                rec("NEW", 5000, 1.0, 1.0, None),
                rec("OLD", 4000, 1.0, 1.0, None),
            ],
            5000,
        )
        .unwrap();
        h.record(&[rec("NEW", 5100, 2.0, 2.0, None)], 5100).unwrap();
        assert_eq!(h.segment_count(), 2);

        // Cut at 200: segment 1 (t 100..150) is deleted; the open one is untouched.
        let report = h.maintain(5200, 200).unwrap();
        assert_eq!(report.points_pruned, 2);
        assert_eq!(report.segments_deleted, 1);
        assert_eq!(h.segment_count(), 1);
        assert!(!dir.join(format!("{:016}.cseg", 1)).exists());
        assert!(dir.join(format!("{:016}.seg", 2)).exists());

        // An hour on, cut at 4500: segment 2 closes and compacts down to NEW's points.
        let report = h.maintain(5000 + SEGMENT_SECS, 4500).unwrap();
        assert_eq!(report.points_pruned, 1);
        assert_eq!(report.segments_compacted, 1);
        assert_eq!(h.point_count(), 2);
        drop(h);
        let h = open(&dir);
        assert_eq!(
            h.point_count(),
            2,
            "the compacted segment holds what remains"
        );
        assert_eq!(
            times(&h.page(&query(0, u64::MAX)).unwrap(), "NEW"),
            [5000, 5100]
        );
        assert_eq!(h.track_count(), 1);

        // Everything older than the cutoff: the segment goes.
        let mut h = h;
        let report = h.maintain(99_999, 9_999).unwrap();
        assert_eq!(report.segments_deleted, 1);
        assert_eq!(h.point_count(), 0);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_decimates_and_filters_select() {
        let mut h = open(&scratch("step"));
        let mut batch: Vec<_> = (0..10)
            .map(|i| rec("A", 100 + i * 5, 1.0, 1.0, None))
            .collect();
        let mut sea = rec("S", 120, 2.0, 2.0, None);
        sea.kind = "sea-track".into();
        sea.layer = "sea".into();
        batch.push(sea);
        h.record(&batch, 0).unwrap();
        let mut q = query(100, 140);
        q.step = 12;
        let page = h.page(&q).unwrap();
        assert_eq!(times(&page, "A"), [100, 115, 130]);
        q.step = 0;
        q.layer = Some("sea".into());
        let page = h.page(&q).unwrap();
        assert_eq!(page.tracks.len(), 1);
        assert_eq!(page.tracks[0].kind, "sea-track");
        q.layer = None;
        q.entity = Some("A".into());
        q.from = 120;
        q.to = 130;
        let page = h.page(&q).unwrap();
        assert_eq!(page.tracks.len(), 1);
        assert_eq!(times(&page, "A"), [120, 125, 130]);
    }

    #[test]
    fn pages_resume_from_the_cursor_without_gaps_or_repeats() {
        let mut h = open(&scratch("page"));
        let mut batch = Vec::new();
        for e in ["A", "B", "C"] {
            for i in 0..50 {
                batch.push(rec(e, 1000 + i * 2, 10.12345, 20.5, Some(100.0)));
            }
        }
        h.record(&batch, 0).unwrap();
        for step in [0, 5] {
            let mut q = query(1000, 1098);
            q.step = step;
            let whole = h.page(&q).unwrap();
            assert_eq!(whole.next, None);
            q.max_bytes = 300;
            let mut seen: BTreeMap<String, Vec<u64>> = BTreeMap::new();
            let mut pages = 0;
            loop {
                let page = h.page(&q).unwrap();
                pages += 1;
                let json = page.to_json(q.from, q.to);
                assert!(json.len() < 300 + 200, "{}", json.len());
                for t in &page.tracks {
                    seen.entry(t.id.clone())
                        .or_default()
                        .extend(t.points.iter().map(|p| p.0));
                }
                match page.next {
                    Some(next) => q.after = Some(next),
                    None => break,
                }
            }
            assert!(pages > 5, "the small budget forced paging ({pages})");
            for t in &whole.tracks {
                assert_eq!(
                    seen[&t.id],
                    t.points.iter().map(|p| p.0).collect::<Vec<_>>(),
                    "step {step}"
                );
            }
        }
        let mut q = query(0, 10);
        q.after = Some("nonsense".into());
        assert!(h.page(&q).is_err());
    }
}
