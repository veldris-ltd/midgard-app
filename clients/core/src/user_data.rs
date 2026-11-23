//! Your own data: JSON files a user imports into the app, Heimdall observations, and the
//! address rule for a gateway or feed the user adds. `[DD-1]` `[RC-10]` `[NFR-SEC-2]`
//! `[AC-C-13]` `[FR-H-1]`
//!
//! **Imports** follow `docs/guides/IMPORT_YOUR_OWN_DATA.md` exactly, with the parsers
//! `mg-admin import` and the HTTP gateway use (`ontology-import`): an `ontology.json`
//! and one or more JSON Lines data files, each checked line by line against the
//! ontology and then against what is already imported, all or nothing. The first bad
//! line refuses the whole import and names the file and the line; an import never
//! replaces an object; a link's ends must be in the import or already imported. Several
//! files are checked in the order given, each against the ones before it, as running
//! `mg-admin import` once per file would.
//!
//! Where the apps differ from an enclosure, for the app's own reasons:
//! - **One ontology.** Imported data has one ontology. A later import names the same
//!   file (or none, to keep it); another ontology is refused until the imported data is
//!   removed, because the objects already imported were checked against the first.
//! - **No clash with the sample data.** An object type whose id the release's sample
//!   data already declares (`aircraft`, `facility`, …) is refused, so an imported
//!   object can never be taken for a sample one.
//! - **The `operations` purpose.** Every login to a gateway is issued for the purpose
//!   `operations`, and so is every persona here: an object whose `purposes` lack it
//!   imports, as with `mg-admin`, but nobody can read it, and it is reported exactly as
//!   if it did not exist. The import's summary counts them, so the user knows.
//!
//! **Observations** are Heimdall's: the files in `samples/4-heimdall-observations/`
//! (`{"feed": …, "sample" | "observations": [{"entity", "kind", "observedAt",
//! "attrs"}]}`), the tab-separated lines `samples/convert.py observations` writes for
//! `heimdall-source` (feed, kind, entity, observed-at, `k=v,…` attributes), and JSON
//! Lines of single observations (what a feed URL may serve). An observation with a
//! finite `lat`/`lon` (or `latitude`/`longitude`) in range is placed on the picture;
//! the others are kept for the feed's row only. None is signed by a certified mesh
//! identity, so none is ever fused into a signed track: the apps show them as
//! **UNVERIFIED FEED** layers, as `heimdall-feed` quarantines an unverified feed.
//!
//! **Addresses** follow the SDK's `assertSafeBaseUrl` (`sdk/ts/src/http/baseUrl.ts`):
//! `https://` anywhere, `http://` only to `127.0.0.1`, `localhost` or `[::1]`, refused
//! before anything is sent, with the SDK's own words.

use std::collections::{BTreeMap, BTreeSet};

use attest::json::{parse as parse_json, Json};
use ontology_import::import_file::{parse_import, Import};
use ontology_import::ontology_file::{parse_ontology, ActionEffect, Ontology};
use pqc_agility::sha256;

use crate::demo::canon;
use crate::marking::{may_see, MarkingLevel, RowMarking};
use crate::source::Grant;

pub use ontology_import::import_file::{LINK_RECORD_KEYS, OBJECT_RECORD_KEYS};

/// The persisted form's `format`.
pub const FORMAT: &str = "midgard-user-data";
/// The one purpose every persona (and every gateway login) holds.
pub const LOGIN_PURPOSE: &str = "operations";
/// The largest file an import takes, in bytes (the importer reads it whole).
pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
/// The most files one import takes.
pub const MAX_FILES: usize = 32;
/// The most observations one layer keeps (the rest are counted, not kept).
pub const MAX_OBSERVATIONS: usize = 10_000;
/// The most attributes one observation keeps.
pub const MAX_ATTRS: usize = 64;
/// The longest attribute value kept, in characters.
pub const MAX_ATTR_CHARS: usize = 200;

/// A file as the user picked it: its name (never its path) and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedText {
    /// The file's name, e.g. `network.jsonl`.
    pub name: String,
    /// Its contents.
    pub text: String,
}

/// What a data file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Object and link records (the import format).
    Records,
    /// Heimdall observations.
    Observations,
}

/// One file of an import, as checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSummary {
    /// Its name.
    pub name: String,
    /// What it held.
    pub kind: FileKind,
    /// Objects it added.
    pub objects: usize,
    /// Links it added.
    pub links: usize,
    /// Observations it held.
    pub observations: usize,
}

/// One Heimdall observation.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// The entity key.
    pub entity: String,
    /// Its kind, e.g. `utility-power-grid`.
    pub kind: String,
    /// When it was observed, Unix seconds.
    pub observed_at: i64,
    /// Its flat attributes, as text, in file order.
    pub attrs: Vec<(String, String)>,
    /// Where it is, when it says so.
    pub position: Option<(f64, f64)>,
}

impl Observation {
    /// The `name` attribute, or the entity key.
    pub fn label(&self) -> &str {
        self.attrs
            .iter()
            .find(|(k, _)| k == "name")
            .map_or(self.entity.as_str(), |(_, v)| v.as_str())
    }
}

/// The observations of one feed.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservationLayer {
    /// The file or feed it came from.
    pub source: String,
    /// The feed the observations name (the file's `feed`, or the source).
    pub feed: String,
    /// The observations kept, at most [`MAX_OBSERVATIONS`].
    pub observations: Vec<Observation>,
    /// How many there were, kept or not.
    pub total: usize,
}

/// An import checked against its ontology and what was imported before: everything
/// the app installs, and the files to seal for next time.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    /// The ontology file's name and text.
    pub ontology_file: NamedText,
    /// It, parsed.
    pub ontology: Ontology,
    /// Objects, in file order.
    pub objects: Vec<fabric::ObjectInstance>,
    /// Links, in file order.
    pub links: Vec<fabric::LinkInstance>,
    /// Observation layers, one per feed.
    pub layers: Vec<ObservationLayer>,
    /// Every data file, in the order imported.
    pub files: Vec<FileSummary>,
    /// The data files' texts, to seal.
    pub texts: Vec<NamedText>,
}

impl Checked {
    /// Objects no one can read: their purposes lack [`LOGIN_PURPOSE`].
    pub fn unreadable(&self) -> usize {
        self.objects.iter().filter(|o| !readable_purpose(o)).count()
    }

    /// The persisted form: the ontology and every data file, as text. Opening it
    /// checks everything again ([`restore`]), so nothing in it is trusted.
    pub fn persisted(&self) -> Vec<u8> {
        let file = |f: &NamedText| {
            Json::Obj(vec![
                ("name".into(), Json::Str(f.name.clone())),
                ("text".into(), Json::Str(f.text.clone())),
            ])
        };
        canon::encode(&Json::Obj(vec![
            ("format".into(), Json::Str(FORMAT.into())),
            ("version".into(), Json::Num("1".into())),
            ("ontology".into(), file(&self.ontology_file)),
            (
                "files".into(),
                Json::Arr(self.texts.iter().map(file).collect()),
            ),
        ]))
        .into_bytes()
    }

    /// SHA-256 of [`persisted`](Self::persisted), in hex: what the import's audit row
    /// names.
    pub fn digest(&self) -> String {
        use std::fmt::Write;
        sha256(&self.persisted())
            .iter()
            .fold(String::new(), |mut out, b| {
                let _ = write!(out, "{b:02x}");
                out
            })
    }
}

fn readable_purpose(o: &fabric::ObjectInstance) -> bool {
    o.allowed_purposes.iter().any(|p| p.0 == LOGIN_PURPOSE)
}

/// What a data file holds, from its first record.
pub fn kind_of(text: &str) -> FileKind {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(doc) = parse_json(trimmed.trim_end()) {
            if doc.get("feed").is_some()
                && (doc.get("sample").is_some() || doc.get("observations").is_some())
            {
                return FileKind::Observations;
            }
        }
    }
    let first = text.lines().map(str::trim).find(|l| !l.is_empty());
    match first {
        Some(l) if l.split('\t').count() == 5 => FileKind::Observations,
        Some(l) => match parse_json(l) {
            Ok(v) if v.get("entity").is_some() && v.get("object").is_none() => {
                FileKind::Observations
            }
            _ => FileKind::Records,
        },
        None => FileKind::Records,
    }
}

/// Check an import: `ontology` (or, with none, the one already imported) and `files`,
/// in order, against what `existing` holds. `reserved` are the object type ids the
/// sample data declares. All or nothing: the first problem refuses the whole import,
/// naming the file and, for a record, the line.
pub fn check(
    existing: Option<&Checked>,
    ontology: Option<&NamedText>,
    files: &[NamedText],
    reserved: &[String],
) -> Result<Checked, String> {
    if files.is_empty() && existing.is_none() {
        return Err("pick one or more data files (.jsonl or .json) to import".into());
    }
    if files.len() > MAX_FILES {
        return Err(format!(
            "an import takes at most {MAX_FILES} files; import the rest in another"
        ));
    }
    for f in files.iter().chain(ontology) {
        if f.text.len() > MAX_FILE_BYTES {
            return Err(format!(
                "{}: larger than {} MiB; split it into several files",
                f.name,
                MAX_FILE_BYTES / (1024 * 1024)
            ));
        }
    }
    let ontology_file = match (ontology, existing) {
        (Some(o), Some(e)) if o.text != e.ontology_file.text => {
            return Err(format!(
                "{}: the imported data already has an ontology ({}); remove the imported \
                 data to import against another",
                o.name, e.ontology_file.name
            ))
        }
        (_, Some(e)) => e.ontology_file.clone(),
        (Some(o), None) => o.clone(),
        (None, None) => {
            return Err(
                "pick the ontology.json your data files are written against, as well as the \
                 data files"
                    .into(),
            )
        }
    };
    let parsed =
        parse_ontology(&ontology_file.text).map_err(|e| format!("{}: {e}", ontology_file.name))?;
    if let Some(t) = parsed
        .object_types
        .iter()
        .find(|t| reserved.iter().any(|r| *r == t.id))
    {
        return Err(format!(
            "{}: object type \"{}\" is already declared by the sample data; give yours \
             another id",
            ontology_file.name, t.id
        ));
    }

    let mut out = match existing {
        Some(e) => e.clone(),
        None => Checked {
            ontology_file: ontology_file.clone(),
            ontology: parsed.clone(),
            objects: Vec::new(),
            links: Vec::new(),
            layers: Vec::new(),
            files: Vec::new(),
            texts: Vec::new(),
        },
    };
    for f in files {
        if out.texts.iter().any(|t| t.name == f.name) {
            return Err(format!(
                "{}: a file of this name is already imported; rename it if it holds new data",
                f.name
            ));
        }
        add_file(&mut out, f, &parsed)?;
    }
    Ok(out)
}

/// Check one data file into `out`.
fn add_file(out: &mut Checked, f: &NamedText, ontology: &Ontology) -> Result<(), String> {
    match kind_of(&f.text) {
        FileKind::Observations => {
            let layers = parse_observations(&f.name, &f.text)?;
            let n = layers.iter().map(|l| l.total).sum();
            out.layers.extend(layers);
            out.files.push(FileSummary {
                name: f.name.clone(),
                kind: FileKind::Observations,
                objects: 0,
                links: 0,
                observations: n,
            });
        }
        FileKind::Records => {
            let import: Import =
                parse_import(&f.text, ontology).map_err(|e| format!("{}: {e}", f.name))?;
            let have: BTreeSet<&str> = out.objects.iter().map(|o| o.id.as_str()).collect();
            import
                .check_against(&|id| have.contains(id))
                .map_err(|e| format!("{}: {}", f.name, e.replace("the enclosure", "this app")))?;
            let known: BTreeSet<(String, String, String)> = out
                .links
                .iter()
                .map(|l| {
                    (
                        l.link_type.clone(),
                        l.source_id.clone(),
                        l.target_id.clone(),
                    )
                })
                .collect();
            let links: Vec<fabric::LinkInstance> = import
                .links
                .into_iter()
                .filter(|l| {
                    !known.contains(&(
                        l.link_type.clone(),
                        l.source_id.clone(),
                        l.target_id.clone(),
                    ))
                })
                .collect();
            out.files.push(FileSummary {
                name: f.name.clone(),
                kind: FileKind::Records,
                objects: import.objects.len(),
                links: links.len(),
                observations: 0,
            });
            out.objects.extend(import.objects);
            out.links.extend(links);
        }
    }
    out.texts.push(f.clone());
    Ok(())
}

/// Read a persisted import back ([`Checked::persisted`]), checking it all again.
pub fn restore(bytes: &[u8], reserved: &[String]) -> Result<Checked, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "saved imported data is not UTF-8")?;
    let doc = parse_json(text).map_err(|e| format!("saved imported data: {e}"))?;
    if doc.get("format").and_then(Json::as_str) != Some(FORMAT) {
        return Err("saved imported data: not imported data".into());
    }
    if doc.get("version") != Some(&Json::Num("1".into())) {
        return Err("saved imported data: unsupported version".into());
    }
    let named = |v: &Json| -> Result<NamedText, String> {
        Ok(NamedText {
            name: v
                .get("name")
                .and_then(Json::as_str)
                .ok_or("saved imported data: a file has no name")?
                .to_string(),
            text: v
                .get("text")
                .and_then(Json::as_str)
                .ok_or("saved imported data: a file has no text")?
                .to_string(),
        })
    };
    let ontology = named(
        doc.get("ontology")
            .ok_or("saved imported data: no ontology")?,
    )?;
    let files = doc
        .get("files")
        .and_then(Json::as_array)
        .ok_or("saved imported data: no files")?
        .iter()
        .map(named)
        .collect::<Result<Vec<_>, _>>()?;
    check(None, Some(&ontology), &files, reserved)
}

// ---- observations ----------------------------------------------------------------------

/// Parse a Heimdall observations file or feed answer (see the module notes): one layer
/// per feed it names, refused whole on the first malformed record, which it names.
pub fn parse_observations(source: &str, text: &str) -> Result<Vec<ObservationLayer>, String> {
    let at = |what: String| format!("{source}: observations refused: {what}");
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(at("the file is empty".into()));
    }
    // One document: {"feed": …, "sample" | "observations": [ … ]}.
    if trimmed.starts_with('{') {
        if let Ok(doc) = parse_json(trimmed) {
            if doc.get("feed").is_some() {
                return document(&doc).map(|l| vec![l]).map_err(at).map(|mut v| {
                    v[0].source = source.to_string();
                    v
                });
            }
        }
    }
    let mut layers: Vec<ObservationLayer> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let here = format!("line {}", index + 1);
        let (feed, o) = if line.contains('\t') {
            tab_line(line).map_err(|e| at(format!("{here}: {e}")))?
        } else {
            let v =
                parse_json(line.trim()).map_err(|e| at(format!("{here}: not valid JSON: {e}")))?;
            let feed = match v.get("feed") {
                None => source.to_string(),
                Some(Json::Str(f)) if !f.trim().is_empty() => f.clone(),
                Some(_) => return Err(at(format!("{here}: \"feed\" must be a non-blank string"))),
            };
            let o = observation(&v, &["feed"]).map_err(|e| at(format!("{here}: {e}")))?;
            (feed, o)
        };
        let layer = match layers.iter_mut().position(|l| l.feed == feed) {
            Some(i) => &mut layers[i],
            None => {
                layers.push(ObservationLayer {
                    source: source.to_string(),
                    feed,
                    observations: Vec::new(),
                    total: 0,
                });
                layers.last_mut().expect("pushed")
            }
        };
        layer.total += 1;
        if layer.observations.len() < MAX_OBSERVATIONS {
            layer.observations.push(o);
        }
    }
    if layers.is_empty() {
        return Err(at("no observation in it".into()));
    }
    Ok(layers)
}

fn document(doc: &Json) -> Result<ObservationLayer, String> {
    let Json::Obj(fields) = doc else {
        return Err("the file must be an object".into());
    };
    for (k, _) in fields {
        if !["feed", "total_in_recording", "sample", "observations"].contains(&k.as_str()) {
            return Err(format!("unknown key \"{k}\""));
        }
    }
    let feed = match doc.get("feed") {
        Some(Json::Str(f)) if !f.trim().is_empty() => f.clone(),
        _ => return Err("\"feed\" must be a non-blank string".into()),
    };
    let list = match (doc.get("sample"), doc.get("observations")) {
        (Some(Json::Arr(a)), None) | (None, Some(Json::Arr(a))) => a,
        (Some(_), Some(_)) => {
            return Err("a file has \"sample\" or \"observations\", not both".into())
        }
        _ => return Err("\"sample\" (or \"observations\") must be a list".into()),
    };
    let mut observations = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let o = observation(item, &[]).map_err(|e| format!("observation {}: {e}", i + 1))?;
        if observations.len() < MAX_OBSERVATIONS {
            observations.push(o);
        }
    }
    Ok(ObservationLayer {
        source: String::new(),
        feed,
        observations,
        total: list.len(),
    })
}

/// One observation object: `entity`, `kind`, `observedAt` and optional flat `attrs`
/// (nested values are left out, as `convert.py observations` leaves them out).
fn observation(v: &Json, extra: &[&str]) -> Result<Observation, String> {
    let Json::Obj(fields) = v else {
        return Err("an observation must be an object".into());
    };
    for (k, _) in fields {
        if !["entity", "kind", "observedAt", "attrs"].contains(&k.as_str())
            && !extra.contains(&k.as_str())
        {
            return Err(format!("unknown key \"{k}\""));
        }
    }
    let entity = match v.get("entity") {
        Some(Json::Str(s)) if !s.trim().is_empty() => s.clone(),
        Some(Json::Num(n)) => n.clone(),
        _ => return Err("\"entity\" must be a non-blank string".into()),
    };
    let kind = match v.get("kind") {
        Some(Json::Str(s)) if !s.trim().is_empty() => s.clone(),
        _ => return Err(format!("{entity}: \"kind\" must be a non-blank string")),
    };
    let observed_at = match v.get("observedAt") {
        Some(Json::Num(n)) => seconds(n)
            .ok_or_else(|| format!("{entity}: \"observedAt\" must be Unix seconds, not {n}"))?,
        None | Some(Json::Null) => 0,
        Some(_) => return Err(format!("{entity}: \"observedAt\" must be Unix seconds")),
    };
    let mut attrs = Vec::new();
    match v.get("attrs") {
        None | Some(Json::Null) => {}
        Some(Json::Obj(pairs)) => {
            for (k, val) in pairs {
                let text = match val {
                    Json::Str(s) => s.clone(),
                    Json::Num(n) => n.clone(),
                    Json::Bool(b) => b.to_string(),
                    Json::Null | Json::Arr(_) | Json::Obj(_) => continue,
                };
                if attrs.len() < MAX_ATTRS {
                    attrs.push((k.clone(), text.chars().take(MAX_ATTR_CHARS).collect()));
                }
            }
        }
        Some(_) => return Err(format!("{entity}: \"attrs\" must be an object")),
    }
    let position = position(&attrs);
    Ok(Observation {
        entity,
        kind,
        observed_at,
        attrs,
        position,
    })
}

/// A `heimdall-source` line: feed, kind, entity, observed-at, `k=v,…` attributes.
fn tab_line(line: &str) -> Result<(String, Observation), String> {
    let cells: Vec<&str> = line.split('\t').collect();
    if cells.len() != 5 {
        return Err(format!(
            "a tab-separated observation has 5 fields (feed, kind, entity, observed-at, \
             attributes), not {}",
            cells.len()
        ));
    }
    let blank = |i: usize, what: &str| -> Result<String, String> {
        let c = cells[i].trim();
        if c.is_empty() {
            Err(format!("the {what} is blank"))
        } else {
            Ok(c.to_string())
        }
    };
    let feed = blank(0, "feed")?;
    let kind = blank(1, "kind")?;
    let entity = blank(2, "entity")?;
    let observed_at = seconds(cells[3].trim())
        .ok_or_else(|| format!("{entity}: the observed-at time must be Unix seconds"))?;
    let mut attrs = Vec::new();
    for pair in cells[4].split(',').filter(|p| !p.trim().is_empty()) {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| format!("{entity}: attribute \"{pair}\" is not k=v"))?;
        if attrs.len() < MAX_ATTRS {
            attrs.push((
                k.trim().to_string(),
                v.trim().chars().take(MAX_ATTR_CHARS).collect(),
            ));
        }
    }
    let position = position(&attrs);
    Ok((
        feed,
        Observation {
            entity,
            kind,
            observed_at,
            attrs,
            position,
        },
    ))
}

fn seconds(text: &str) -> Option<i64> {
    let n: i64 = text.parse().ok()?;
    (n >= 0).then_some(n)
}

/// A finite position in range from `lat`/`lon` or `latitude`/`longitude`.
pub fn position(attrs: &[(String, String)]) -> Option<(f64, f64)> {
    let get = |names: &[&str]| {
        attrs
            .iter()
            .find(|(k, _)| names.contains(&k.to_ascii_lowercase().as_str()))
            .and_then(|(_, v)| v.parse::<f64>().ok())
            .filter(|x| x.is_finite())
    };
    let lat = get(&["lat", "latitude"])?;
    let lon = get(&["lon", "lng", "longitude"])?;
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some((lat, lon))
}

// ---- addresses -------------------------------------------------------------------------

const LOOPBACK_HOSTS: [&str; 4] = ["127.0.0.1", "localhost", "[::1]", "::1"];

/// An address the user typed, split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    /// `https` or `http`.
    pub scheme: String,
    /// The host, lowercase; an IPv6 literal keeps its brackets.
    pub host: String,
    /// The port, when given.
    pub port: Option<u16>,
    /// The path and query, without a trailing `/`.
    pub rest: String,
}

impl Address {
    /// `scheme://host[:port]rest`.
    pub fn url(&self) -> String {
        match self.port {
            Some(p) => format!("{}://{}:{p}{}", self.scheme, self.host, self.rest),
            None => format!("{}://{}{}", self.scheme, self.host, self.rest),
        }
    }

    /// Whether the host is the loopback interface.
    pub fn is_loopback(&self) -> bool {
        LOOPBACK_HOSTS.contains(&self.host.as_str())
    }
}

fn split(url: &str) -> Option<Address> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return Some(Address {
            scheme,
            host: String::new(),
            port: None,
            rest: String::new(),
        });
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.is_empty() || authority.contains('@') || tail.contains('#') {
        return None;
    }
    if authority
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
        || tail.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (inside, after) = v6.split_once(']')?;
        if inside.is_empty()
            || !inside
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        {
            return None;
        }
        let port = match after {
            "" => None,
            p => Some(p.strip_prefix(':')?.parse::<u16>().ok()?),
        };
        (format!("[{}]", inside.to_ascii_lowercase()), port)
    } else {
        let (h, p) = match authority.rsplit_once(':') {
            Some((h, p)) => (h, Some(p.parse::<u16>().ok()?)),
            None => (authority, None),
        };
        if h.is_empty()
            || !h
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        {
            return None;
        }
        (h.to_ascii_lowercase(), p)
    };
    Some(Address {
        scheme,
        host,
        port,
        rest: tail.trim_end_matches('/').to_string(),
    })
}

/// The SDK's rule for a gateway address (`assertSafeBaseUrl`), with its words: the
/// address to call, `…/api/v1` when the user gave the host alone.
pub fn check_gateway_address(url: &str) -> Result<Address, String> {
    let mut a = split(url)
        .filter(|a| !a.host.is_empty() || a.scheme != "https" && a.scheme != "http")
        .ok_or_else(|| format!("Refusing a gateway URL that does not parse: {url}"))?;
    if a.scheme == "https" || (a.scheme == "http" && a.is_loopback()) {
        if a.rest.contains('?') {
            return Err(format!("Refusing a gateway URL that does not parse: {url}"));
        }
        if a.rest.is_empty() {
            a.rest = "/api/v1".into();
        }
        return Ok(a);
    }
    if a.scheme != "https" && a.scheme != "http" {
        return Err(format!("Refusing a gateway URL that does not parse: {url}"));
    }
    Err("Refusing a plaintext gateway URL; use https:// unless the edge is on loopback".into())
}

/// The same rule for a feed URL: `https://` anywhere, `http://` only on loopback.
pub fn check_feed_address(url: &str) -> Result<Address, String> {
    let a = split(url)
        .filter(|a| !a.host.is_empty())
        .ok_or_else(|| format!("Refusing a feed URL that does not parse: {url}"))?;
    if a.scheme == "https" || (a.scheme == "http" && a.is_loopback()) {
        return Ok(a);
    }
    if a.scheme != "http" {
        return Err(format!("Refusing a feed URL that does not parse: {url}"));
    }
    Err("Refusing a plaintext feed URL; use https:// unless the feed is on loopback".into())
}

// ---- installed in the sample source ----------------------------------------------------

/// The object types collection.
pub const TYPES: &str = "midgard.objectTypes";
/// The objects collection.
pub const OBJECTS: &str = "midgard.objects";
/// Heimdall's feeds collection.
pub const FEEDS: &str = "heimdall.feeds";
/// The prefix of a user-added feed row's id.
pub const FEED_ROW_PREFIX: &str = "user:";

/// A feed row the user added (an imported observations file or a feed URL), for
/// Heimdall's Feeds data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFeedRow {
    /// A stable id (made unique with [`FEED_ROW_PREFIX`]).
    pub id: String,
    /// The name the user gave, or the file's.
    pub name: String,
    /// `observations file` or `feed URL`.
    pub kind: String,
    /// How often it is read: `file`, `every 60 s`.
    pub rate: String,
    /// The state the screen shows: `UNVERIFIED FEED`, or the last error.
    pub status_label: String,
    /// Whether the last fetch failed.
    pub failing: bool,
}

/// What the sample source holds of the user's own data. `[AC-C-13]`
#[derive(Debug, Default)]
pub struct UserTables {
    /// The import installed, if any.
    pub installed: Option<Checked>,
    /// An import checked and waiting for its audited action (`midgard.importData`).
    pub staged: Option<Checked>,
    /// Imported object type ids.
    pub type_ids: BTreeSet<String>,
}

impl UserTables {
    /// The installed action declared under the title `title`, if it is one of the import's.
    pub fn action(&self, title: &str) -> Option<&ontology_import::ontology_file::ActionDecl> {
        self.installed
            .as_ref()?
            .ontology
            .actions
            .iter()
            .find(|a| a.title == title || a.id == title)
    }
}

fn level_of(m: &policy::Marking) -> MarkingLevel {
    MarkingLevel::ALL[usize::from(m.level.0).min(MarkingLevel::ALL.len() - 1)]
}

fn row_marking(m: &policy::Marking) -> RowMarking {
    RowMarking::new(level_of(m), m.compartments.iter().map(|c| c.0.clone()))
}

fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

fn n(v: usize) -> Json {
    Json::Num(v.to_string())
}

fn obj(pairs: Vec<(&str, Json)>) -> Json {
    Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// An object as the gateway shows it (`object_json`): properties as strings.
pub fn object_item(o: &fabric::ObjectInstance) -> Json {
    let level = level_of(&o.marking).as_str();
    let compartments: Vec<String> = o.marking.compartments.iter().map(|c| c.0.clone()).collect();
    let name = if compartments.is_empty() {
        level.to_uppercase()
    } else {
        format!("{} // {}", level.to_uppercase(), compartments.join("/"))
    };
    obj(vec![
        ("id", s(&o.id)),
        ("typeId", s(&o.type_id)),
        (
            "propertyValues",
            Json::Obj(
                o.properties
                    .iter()
                    .map(|(k, v)| (k.clone(), Json::Str(v.as_index_key())))
                    .collect(),
            ),
        ),
        (
            "markings",
            Json::Arr(vec![obj(vec![
                ("id", s(&format!("mk-{level}"))),
                ("name", s(&name)),
                ("level", s(level)),
                (
                    "compartments",
                    Json::Arr(compartments.iter().map(|c| s(c)).collect()),
                ),
            ])]),
        ),
    ])
}

fn link_label(ontology: &Ontology, l: &fabric::LinkType) -> String {
    let target = ontology
        .object_types
        .iter()
        .find(|t| t.id == l.target_type)
        .map_or(l.target_type.as_str(), |t| t.title.as_str());
    format!("{} → {target}", l.api_name)
}

/// An object type's row as the gateway lists it (`object_types`), counted over the
/// objects `grant` may read: the instance count, the highest marking and the
/// compartments among them.
pub fn type_item(c: &Checked, type_id: &str, grant: Option<&Grant>) -> Option<Json> {
    let t = c.ontology.object_types.iter().find(|t| t.id == type_id)?;
    let readable: Vec<&fabric::ObjectInstance> = c
        .objects
        .iter()
        .filter(|o| o.type_id == type_id && readable_purpose(o))
        .filter(|o| grant.is_some_and(|g| may_see(g.clearance(), &row_marking(&o.marking))))
        .collect();
    let top = readable
        .iter()
        .map(|o| level_of(&o.marking))
        .max()
        .unwrap_or(MarkingLevel::Open);
    let compartments: BTreeSet<&str> = readable
        .iter()
        .flat_map(|o| o.marking.compartments.iter())
        .map(|c| c.0.as_str())
        .collect();
    let files: Vec<&str> = c
        .files
        .iter()
        .filter(|f| f.kind == FileKind::Records)
        .map(|f| f.name.as_str())
        .collect();
    Some(obj(vec![
        ("name", s(&t.title)),
        ("apiName", s(&t.id)),
        ("propertyCount", n(t.properties.len())),
        (
            "properties",
            Json::Arr(
                t.properties
                    .iter()
                    .map(|p| {
                        obj(vec![
                            ("name", s(&p.api_name)),
                            ("type", s(p.value_type.as_str())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("instanceCount", s(&readable.len().to_string())),
        ("marking", s(top.as_str())),
        (
            "compartment",
            if compartments.is_empty() {
                Json::Null
            } else {
                s(&compartments.into_iter().collect::<Vec<_>>().join("/"))
            },
        ),
        (
            "links",
            Json::Arr(
                c.ontology
                    .link_types
                    .iter()
                    .filter(|l| l.source_type == t.id)
                    .map(|l| obj(vec![("label", s(&link_label(&c.ontology, l)))]))
                    .collect(),
            ),
        ),
        (
            "actions",
            Json::Arr(
                c.ontology
                    .actions
                    .iter()
                    .filter(|a| a.object_type == t.id)
                    .map(|a| {
                        obj(vec![
                            ("name", s(&a.title)),
                            ("humanGated", Json::Bool(a.human_gated)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("updated", s(&format!("imported · {}", files.join(", ")))),
    ]))
}

/// A user feed's row in Heimdall's feeds, as `HeimdallFeed`.
pub fn feed_item(f: &UserFeedRow) -> Json {
    obj(vec![
        ("name", s(&f.name)),
        ("kind", s(&f.kind)),
        ("rate", s(&f.rate)),
        ("latency", s("—")),
        ("weight", s("low")),
        // Not signed by a certified mesh identity: never authenticated, never fused.
        ("auth", Json::Bool(false)),
        ("scheme", s("none")),
        ("status", s(if f.failing { "danger" } else { "warn" })),
        ("statusLabel", s(&f.status_label)),
    ])
}

/// The search-around graph over imported objects and links, as the gateway walks it
/// (`search_around`): links in both directions, up to `hops`, from one object or every
/// object of a type (`<type>/*`), only objects `grant` may read; the others are
/// counted in `withheld`, never shown.
pub fn search_around(c: &Checked, grant: &Grant, seed: &str, hops: u64, limit: usize) -> Json {
    let visible = |o: &fabric::ObjectInstance| {
        readable_purpose(o) && may_see(grant.clearance(), &row_marking(&o.marking))
    };
    let by_id: BTreeMap<&str, &fabric::ObjectInstance> =
        c.objects.iter().map(|o| (o.id.as_str(), o)).collect();
    let title = |type_id: &str| {
        c.ontology
            .object_types
            .iter()
            .find(|t| t.id == type_id)
            .map_or(type_id.to_string(), |t| t.title.clone())
    };
    let seeds: Vec<&str> = match seed.strip_suffix("/*") {
        Some(t) => c
            .objects
            .iter()
            .filter(|o| o.type_id == t)
            .map(|o| o.id.as_str())
            .collect(),
        None => by_id.get(seed).map(|o| o.id.as_str()).into_iter().collect(),
    };
    let mut hop_of: BTreeMap<&str, u64> = BTreeMap::new();
    let mut withheld: BTreeSet<&str> = BTreeSet::new();
    let mut frontier: Vec<&str> = Vec::new();
    for id in &seeds {
        if by_id.get(id).is_some_and(|o| visible(o)) {
            hop_of.insert(id, 0);
            frontier.push(id);
        } else {
            withheld.insert(id);
        }
    }
    let seed_objects = frontier.len();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut truncated = false;
    for hop in 1..=hops {
        let mut next = Vec::new();
        for id in &frontier {
            for l in &c.links {
                let other = if l.source_id == *id {
                    l.target_id.as_str()
                } else if l.target_id == *id {
                    l.source_id.as_str()
                } else {
                    continue;
                };
                let Some(o) = by_id.get(other) else { continue };
                if !visible(o) {
                    withheld.insert(other);
                    continue;
                }
                if !hop_of.contains_key(other) {
                    if hop_of.len() >= limit {
                        truncated = true;
                        continue;
                    }
                    hop_of.insert(other, hop);
                    next.push(other);
                }
                let label = c
                    .ontology
                    .link_types
                    .iter()
                    .find(|t| t.id == l.link_type)
                    .map_or(l.link_type.clone(), |t| link_label(&c.ontology, t));
                let e = (l.source_id.clone(), l.target_id.clone(), label);
                if !edges.contains(&e) {
                    edges.push(e);
                }
            }
        }
        frontier = next;
    }
    edges.retain(|(f, t, _)| hop_of.contains_key(f.as_str()) && hop_of.contains_key(t.as_str()));
    let mut nodes: Vec<(&str, u64)> = hop_of.into_iter().collect();
    nodes.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(b.0)));
    obj(vec![
        ("seed", s(seed)),
        (
            "nodes",
            Json::Arr(
                nodes
                    .iter()
                    .filter_map(|(id, hop)| by_id.get(id).map(|o| (o, *hop)))
                    .map(|(o, hop)| {
                        let label = match o.properties.get("name") {
                            Some(fabric::Value::Text(name)) => name.clone(),
                            _ => o.id.clone(),
                        };
                        let item = object_item(o);
                        obj(vec![
                            ("id", s(&o.id)),
                            ("label", s(&label)),
                            ("typeName", s(&title(&o.type_id))),
                            ("marking", s(level_of(&o.marking).as_str())),
                            ("hop", Json::Num(hop.to_string())),
                            (
                                "propertyValues",
                                item.get("propertyValues")
                                    .cloned()
                                    .unwrap_or(Json::Obj(vec![])),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "edges",
            Json::Arr(
                edges
                    .into_iter()
                    .map(|(f, t, l)| obj(vec![("from", s(&f)), ("to", s(&t)), ("label", s(&l))]))
                    .collect(),
            ),
        ),
        ("truncated", Json::Bool(truncated)),
        ("nodeLimit", n(limit)),
        ("seedObjects", n(seed_objects)),
        ("withheld", n(withheld.len())),
    ])
}

/// Apply an imported action's effect to an object's row (`set-property` or
/// `cycle-property`), as the gateway's fabric applies it.
pub fn apply_effect(item: &mut Json, effect: &ActionEffect) {
    let Json::Obj(pairs) = item else { return };
    let Some((_, Json::Obj(props))) = pairs.iter_mut().find(|(k, _)| k == "propertyValues") else {
        return;
    };
    let (property, value) = match effect {
        ActionEffect::SetProperty { property, value } => (property, value.clone()),
        ActionEffect::CycleProperty { property, values } => {
            let current = props
                .iter()
                .find(|(k, _)| k == property)
                .and_then(|(_, v)| v.as_str());
            let next = match values.iter().position(|v| Some(v.as_str()) == current) {
                Some(i) => values[(i + 1) % values.len()].clone(),
                None => values[0].clone(),
            };
            (property, next)
        }
    };
    match props.iter_mut().find(|(k, _)| k == property) {
        Some((_, v)) => *v = Json::Str(value),
        None => props.push((property.clone(), Json::Str(value))),
    }
}

// ---- a gateway's answers as rows -------------------------------------------------------

/// A gateway's list answer (a JSON array of read-model items, e.g. `GET /object-types`)
/// as delivered rows, for the read-model decoders to check strictly. Each row's id is
/// its `id` (or `apiName`, or its position), and its marking the one the item states
/// (`marking`, or the highest of `markings`), `open` when it states none: the gateway
/// has already scoped the answer to the signed-in user, so nothing here is withheld.
/// No audit row is claimed as verified: this device checked no signature.
pub fn gateway_delivery(text: &str) -> Result<Vec<crate::source::Delivered>, String> {
    let doc = parse_json(text).map_err(|e| format!("the gateway's answer is not JSON: {e}"))?;
    let items = doc.as_array().ok_or("the gateway's answer is not a list")?;
    Ok(items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let id = ["id", "apiName", "name"]
                .iter()
                .find_map(|k| item.get(k).and_then(Json::as_str))
                .map_or_else(|| i.to_string(), str::to_string);
            let stated = |m: &Json| -> Option<RowMarking> {
                let level = MarkingLevel::parse(m.get("level").and_then(Json::as_str)?)?;
                let comps = m
                    .get("compartments")
                    .and_then(Json::as_array)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect::<Vec<_>>();
                Some(RowMarking::new(level, comps))
            };
            let marking = item
                .get("markings")
                .and_then(Json::as_array)
                .and_then(|ms| ms.iter().filter_map(stated).max_by_key(|m| m.level()))
                .or_else(|| {
                    item.get("marking")
                        .and_then(Json::as_str)
                        .and_then(MarkingLevel::parse)
                        .map(|l| RowMarking::new(l, Vec::<String>::new()))
                })
                .unwrap_or_else(|| RowMarking::new(MarkingLevel::Open, Vec::<String>::new()));
            crate::source::Delivered::Row {
                id,
                marking,
                body: canon::encode(item).into_bytes(),
                signature: None,
            }
        })
        .collect())
}

/// Leave out of the gateway row at `path` (`$[<id>]`, a row itself, not a nested
/// value) the key `key` its read model does not declare: a gateway may add fields
/// beyond the console contract (the audit row's `outcome`), which the web console's
/// TypeScript ignores. Nested values stay strict. Whether a row was changed.
pub fn drop_extra_key(rows: &mut [crate::source::Delivered], path: &str, key: &str) -> bool {
    for d in rows.iter_mut() {
        let crate::source::Delivered::Row { id, body, .. } = d else {
            continue;
        };
        if format!("$[{id}]") != path {
            continue;
        }
        let Some(Json::Obj(pairs)) = std::str::from_utf8(body)
            .ok()
            .and_then(|t| parse_json(t).ok())
        else {
            return false;
        };
        let before = pairs.len();
        let kept: Vec<(String, Json)> = pairs.into_iter().filter(|(k, _)| k != key).collect();
        if kept.len() == before {
            return false;
        }
        *body = canon::encode(&Json::Obj(kept)).into_bytes();
        return true;
    }
    false
}
