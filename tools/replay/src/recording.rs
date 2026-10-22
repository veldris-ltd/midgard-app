//! A recording on disk (format version 1, see the phase 2 plan). [design §2]
use service_runtime::json::{parse, Json};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub enum Event {
    /// A Heimdall batch: the feed it came from and its observations as recorded.
    Batch {
        offset: u64,
        feed: String,
        observations: Vec<Json>,
    },
    /// A Forge source file version: written as `file` at `offset`.
    Forge {
        offset: u64,
        file: String,
        path: PathBuf,
    },
}

impl Event {
    pub fn offset(&self) -> u64 {
        match self {
            Event::Batch { offset, .. } | Event::Forge { offset, .. } => *offset,
        }
    }
}

pub struct Recording {
    pub recorded_from: i64,
    pub duration: u64,
    /// Every event, in offset order (batches before Forge files at the same offset).
    pub events: Vec<Event>,
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The batches file, through macOS's gzip when compressed (no compression crate needed).
fn heimdall_lines(dir: &Path) -> Result<String, String> {
    let gz = dir.join("heimdall.jsonl.gz");
    if gz.exists() {
        let out = Command::new("/usr/bin/gzip")
            .arg("-dc")
            .arg(&gz)
            .output()
            .map_err(|e| format!("gzip: {e}"))?;
        if !out.status.success() {
            return Err(format!("gzip could not read {}", gz.display()));
        }
        return String::from_utf8(out.stdout).map_err(|_| "heimdall.jsonl.gz is not UTF-8".into());
    }
    let plain = dir.join("heimdall.jsonl");
    if plain.exists() {
        read(&plain)
    } else {
        Ok(String::new())
    }
}

/// A non-negative offset in seconds; `what` names the entry in the error.
fn offset(v: &Json, what: &str) -> Result<u64, String> {
    match v.get("offset").and_then(Json::as_i64) {
        Some(n) if n >= 0 => Ok(n as u64),
        Some(n) => Err(format!("{what}: negative offset {n}")),
        None => Err(format!("{what}: no offset")),
    }
}

/// A name later used as a file name (a Forge file, a feed's identity file): no
/// directories, not hidden.
fn plain_name<'a>(name: &'a str, what: &str) -> Result<&'a str, String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.starts_with('.') {
        return Err(format!("{what} {name} is not a plain name"));
    }
    Ok(name)
}

/// A path inside the recording: relative, with no `..` component.
fn inside(dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel);
    let ok = !rel.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !ok {
        return Err(format!("forge path {rel} is outside the recording"));
    }
    Ok(dir.join(p))
}

impl Recording {
    pub fn load(dir: &Path) -> Result<Recording, String> {
        let meta = parse(&read(&dir.join("meta.json"))?)?;
        if meta.get("version").and_then(Json::as_i64) != Some(1) {
            return Err("unsupported recording version (this replay reads version 1)".into());
        }
        let recorded_from = meta
            .get("recordedFrom")
            .and_then(Json::as_i64)
            .ok_or("meta.json: no recordedFrom")?;
        let duration = meta
            .get("duration")
            .and_then(Json::as_i64)
            .filter(|d| *d > 0)
            .ok_or("meta.json: no duration")? as u64;
        let mut events = Vec::new();
        for (n, line) in heimdall_lines(dir)?.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let what = format!("heimdall batch {}", n + 1);
            let b = parse(line).map_err(|e| format!("{what}: {e}"))?;
            let offset = offset(&b, &what)?;
            let feed = b
                .str_field("feed")
                .ok_or_else(|| format!("{what}: no feed"))?;
            let feed = plain_name(feed, "feed name")?.to_string();
            let observations = match b.get("observations") {
                Some(Json::Arr(a)) => a.clone(),
                _ => Vec::new(),
            };
            events.push(Event::Batch {
                offset,
                feed,
                observations,
            });
        }
        let manifest = dir.join("forge/manifest.json");
        if manifest.exists() {
            let Json::Arr(items) = parse(&read(&manifest)?)? else {
                return Err("forge/manifest.json is not a list".into());
            };
            for (n, it) in items.iter().enumerate() {
                let what = format!("forge entry {}", n + 1);
                let offset = offset(it, &what)?;
                let file = it
                    .str_field("file")
                    .ok_or_else(|| format!("{what}: no file"))?;
                let file = plain_name(file, "forge file name")?.to_string();
                let rel = it
                    .str_field("path")
                    .ok_or_else(|| format!("{what}: no path"))?;
                let path = inside(dir, rel)?;
                events.push(Event::Forge { offset, file, path });
            }
        }
        // Stable: batches keep file order, then Forge entries, within an offset.
        events.sort_by_key(|e| (e.offset(), matches!(e, Event::Forge { .. })));
        Ok(Recording {
            recorded_from,
            duration,
            events,
        })
    }
}
