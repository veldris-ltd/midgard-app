//! A recording on disk (format version 1, see the phase 2 plan). [design §2]
use service_runtime::json::{parse, Json};
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// Unix seconds a recording's start must lie between (as the time shifting assumes).
const MIN_UNIX: i64 = 1_000_000_000;
const MAX_UNIX: i64 = 4_000_000_000;

pub enum Event {
    /// A Heimdall batch: the feed it came from and its observations as recorded, one
    /// encoded JSON object per line (kept as text: resident memory is about file size).
    Batch {
        offset: u64,
        feed: String,
        lines: String,
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

/// The gzip a compressed batches file (`heimdall.jsonl.gz`) is read through, or `None`
/// on Windows, which has none: recordings for Windows ship uncompressed, as plain
/// `heimdall.jsonl` (docs/plans/2026-10-23-windows-app-design.md).
#[cfg(unix)]
pub const GUNZIP: Option<&str> = Some("/usr/bin/gzip");
#[cfg(not(unix))]
pub const GUNZIP: Option<&str> = None;

/// Call `each` with every line of the batches file (numbered from 1), streamed
/// through `gunzip` when compressed, so the whole file is never held at once. A
/// compressed file with no `gunzip` is refused, saying to unpack it.
fn heimdall_lines(
    dir: &Path,
    gunzip: Option<&str>,
    mut each: impl FnMut(usize, &str) -> Result<(), String>,
) -> Result<(), String> {
    let mut feed = |r: &mut dyn BufRead| -> Result<(), String> {
        for (n, line) in r.lines().enumerate() {
            let line = line.map_err(|e| format!("heimdall batch {}: {e}", n + 1))?;
            each(n + 1, &line)?;
        }
        Ok(())
    };
    let gz = dir.join("heimdall.jsonl.gz");
    if gz.exists() {
        let Some(gunzip) = gunzip else {
            return Err(format!(
                "{}: Windows reads plain heimdall.jsonl; unpack the recording \
                 (gzip -d heimdall.jsonl.gz) before playing it",
                gz.display()
            ));
        };
        let mut child = Command::new(gunzip)
            .arg("-dc")
            .arg(&gz)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("gzip: {e}"))?;
        let mut out = BufReader::new(child.stdout.take().ok_or("gzip: no stdout")?);
        let r = feed(&mut out);
        if r.is_err() {
            let _ = child.kill();
        }
        drop(out);
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        let status = child.wait().map_err(|e| format!("gzip: {e}"))?;
        r?;
        if !status.success() {
            return Err(format!(
                "gzip could not read {}: {}",
                gz.display(),
                err.trim()
            ));
        }
        return Ok(());
    }
    let plain = dir.join("heimdall.jsonl");
    if !plain.exists() {
        return Ok(());
    }
    let f = std::fs::File::open(&plain).map_err(|e| format!("{}: {e}", plain.display()))?;
    feed(&mut BufReader::new(f))
}

/// An offset in seconds within the recording (0 to duration - 1); `what` names the
/// entry in the error.
fn offset(v: &Json, duration: u64, what: &str) -> Result<u64, String> {
    match v.get("offset").and_then(Json::as_i64) {
        Some(n) if n < 0 => Err(format!("{what}: negative offset {n}")),
        Some(n) if n as u64 >= duration => Err(format!(
            "{what}: offset {n} is not within the duration {duration}"
        )),
        Some(n) => Ok(n as u64),
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
    let path = dir.join(p);
    if !path.is_file() {
        return Err(format!("forge path {rel} is not a file"));
    }
    Ok(path)
}

impl Recording {
    /// Load the recording in `dir`, reading a compressed batches file through this
    /// platform's `GUNZIP`.
    pub fn load(dir: &Path) -> Result<Recording, String> {
        Self::load_with(dir, GUNZIP)
    }

    /// `load` with the given gzip, or with none (as on Windows).
    pub fn load_with(dir: &Path, gunzip: Option<&str>) -> Result<Recording, String> {
        let meta = parse(&read(&dir.join("meta.json"))?)?;
        if meta.get("version").and_then(Json::as_i64) != Some(1) {
            return Err("unsupported recording version (this replay reads version 1)".into());
        }
        let recorded_from = meta
            .get("recordedFrom")
            .and_then(Json::as_i64)
            .filter(|t| (MIN_UNIX..=MAX_UNIX).contains(t))
            .ok_or("meta.json: no recordedFrom Unix time")?;
        let duration = meta
            .get("duration")
            .and_then(Json::as_i64)
            .filter(|d| *d > 0)
            .ok_or("meta.json: no positive duration")? as u64;
        let mut events = Vec::new();
        heimdall_lines(dir, gunzip, |n, line| {
            if line.trim().is_empty() {
                return Ok(());
            }
            let what = format!("heimdall batch {n}");
            let b = parse(line).map_err(|e| format!("{what}: {e}"))?;
            let offset = offset(&b, duration, &what)?;
            let feed = b
                .str_field("feed")
                .ok_or_else(|| format!("{what}: no feed"))?;
            let feed = plain_name(feed, "feed name")?.to_string();
            let Some(Json::Arr(observations)) = b.get("observations") else {
                return Err(format!("{what}: observations is not a list"));
            };
            let mut lines = String::new();
            for o in observations {
                if !matches!(o, Json::Obj(_)) {
                    return Err(format!("{what}: an observation is not an object"));
                }
                lines.push_str(&o.encode());
                lines.push('\n');
            }
            if !lines.is_empty() {
                events.push(Event::Batch {
                    offset,
                    feed,
                    lines,
                });
            }
            Ok(())
        })?;
        let manifest = dir.join("forge/manifest.json");
        if manifest.exists() {
            let Json::Arr(items) = parse(&read(&manifest)?)? else {
                return Err("forge/manifest.json is not a list".into());
            };
            for (n, it) in items.iter().enumerate() {
                let what = format!("forge entry {}", n + 1);
                let offset = offset(it, duration, &what)?;
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
        if events.is_empty() {
            return Err("the recording has no events".into());
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
