# Mac app phase 2: recorder and midgard-replay — implementation plan

**Goal:** Record an hour of real Heimdall feed batches and Forge source files on the development Mac, and play it back on a loop through the real signed ingest path, times shifted to now, so the native suite (phase 1) runs without the internet.

**Architecture:** A Python recorder (`tools/mac-app/record.py`, development Mac only) captures the batch files the live feeds already write just before each send, plus Forge's CSVs, then packs them (thinned, cameras dropped, gzipped) into a recording directory outside the suite folder. A new Rust crate, `tools/replay` (binary `midgard-replay`, no new dependencies), loops over the recording: each Heimdall batch, times shifted, is piped into the real `heimdall-source --batch -` under its feed's identity; each Forge CSV version, times shifted, is written atomically where `forge-worker` reads it. `run-native.sh` starts it when `MIDGARD_REPLAY` names a recording.

**Tech Stack:** Rust 1.82 (workspace crate, `service-runtime` for args and JSON), Python 3 standard library, bash, macOS `/usr/bin/gzip`.

Design: `docs/plans/2026-10-22-mac-presentation-app-design.md` §2. Phase 1: `docs/plans/2026-10-22-mac-app-phase1-native-services.md`.

---

## Background the implementer needs

- **How live feeds send.** `~/Desktop/Midgard/heimdall-poc/feeds/feeds.py` (not in git) writes each batch to `heimdall-poc/feeds/out/<feed>.jsonl` (one observation per line, only new or changed items, file rewritten every send) and then runs `heimdall-source --service <feed> --batch <file>`. adsb.lol is sent separately by `heimdall-poc/live_loop.sh` from `heimdall-poc/batch.jsonl` (feed `adsb-lol`). An observation line is
  `{"entity":"406A9F","kind":"air-track","observedAt":1792169458,"attrs":{...}}`.
  `heimdall-source` gives each observation the id `FEED-ENTITY-OBSERVEDAT` and Heimdall refuses a repeated id, so shifting `observedAt` gives fresh ids every loop.
- **"Only new or changed."** feeds.py remembers what it sent in `heimdall-poc/feeds/state/last_sent/<feed>.json`. Clearing those files and restarting the runner (`heimdall-poc/feeds/stop.sh`, `start.sh`) makes every source run at once and send everything. The recorder does that at the start, so the hour contains every item at least once (reference data such as power lines and airspace included).
- **Recording hook in feeds.py.** Several sources write the same feed file about a second apart (`out/regional-extra.jsonl`), so polling `out/` loses batches. feeds.py therefore has a recording hook (see "The feeds.py hook" below). The recorder writes the hook file before restarting the runner and always removes it when capture ends. adsb.lol (`batch.jsonl`, one writer) does not go through feeds.py and is still polled.

## The feeds.py hook

feeds.py is not in git, so its contract with `tools/mac-app/record.py` is recorded here (and at the top of record.py):

- **File:** `$H/heimdall-poc/feeds/state/record_to`.
- **Format:** `"<absolute directory>\npid=<recorder pid>\n"`; the recorder writes its own `os.getpid()`.
- **Copies:** while the file names an existing directory and that pid is alive, feeds.py also copies every batch it sends there as `<time_ns>-<feed>.jsonl`, written first as `.<time_ns>-<feed>.jsonl` and then renamed, so a file without the leading dot is complete.
- **Stale hook:** if the pid is dead or the directory is missing, feeds.py removes the hook file itself, so a killed recorder cannot leave copies piling up. A failed copy never stops the live send.
- **heimdall-source CLI:** `heimdall-source --identity-dir DIR --addr HOST:PORT --service FEED --batch -` reads lines from standard input, prints one line `batch: N accepted, M refused, K unreadable` and exits 1 if anything was refused. Identities for every feed are in `$H/airports-mesh/feeds`. The native Heimdall listens on `127.0.0.1:7730`.
- **Forge:** `forge-worker` re-reads each job's CSV in full on every run (`live-feeds/data/<job>.csv`, jobs in `live-feeds/data/jobs.json`, no checkpoints for file sources). Time columns are Unix seconds: `observed_at`, `measured_at`, `updated_at`, `occurred_at`, `time_raised`, `time_changed`. Fields can be quoted and contain commas (floods' `message`).
- **JSON in Rust:** no serde. Use `service_runtime::json::{parse, Json}` (`Json::Int`, `Num`, `Str`, `Obj(Vec<(String, Json)>)`, `encode()`), and `service_runtime::Args::parse(&argv, &[valued...], &[flags...])`.
- **Build and test natively:** `export PATH=/opt/homebrew/opt/rustup/bin:$PATH` and `export CARGO_TARGET_DIR=$HOME/Library/Caches/midgard-build`; all cargo commands use `--offline`. Never build into the repo: the suite folder (`~/Desktop/Midgard`) must stay within 3.5 GB.
- **Storage:** recordings go in `$HOME/Library/Application Support/Midgard-dev/recordings/<name>` (outside the suite). Measure the packed size in Task 8 and report it.
- **Commits:** signed (`git commit -S`), on `md_mixedfeeds_3d`, ending with the `Co-Authored-By` line. Push after each task.

## The recording format (version 1)

```
<recording>/
  meta.json            {"version":1,"recordedFrom":<unix>,"duration":<s>,"feeds":[...],"forgeFiles":[...]}
  heimdall.jsonl.gz    one batch per line: {"offset":<s>,"feed":"adsb-fi","observations":[{obs},...]}, offset order
  forge/manifest.json  [{"offset":<s>,"file":"aircraft.csv","path":"forge/000000-aircraft.csv"}, ...], offset order
  forge/<offset>-<file>.csv
```

Offsets are seconds from `recordedFrom`. Forge has a version of every file at offset 0 (copied at the start).

## Time shifting rules (both Heimdall and Forge)

`shift = loopStart - recordedFrom`, where `loopStart = replayStart + loop * duration`.
- Time keys (one rule for Heimdall attributes and Forge CSV columns, `is_time_key`): a key or header name that ends with `_at`, `_time`, `_start` or `_end`, starts with `time_`, or is one of `started ended etr valid_from valid_to published`.
- Heimdall: `observedAt += shift`, and each attribute with a time key is shifted when its value is a number between 1,000,000,000 and 4,000,000,000.
- Forge CSV: in each column whose header name (without a leading BOM, surrounding spaces or quotes) is a time key, a field made only of ASCII digits with a value in the same range is shifted. Every other byte is kept, each record's own line terminator (CRLF, LF or none) included.
- Times written as text (dates in names, keys such as `KP-1792022400`, METAR text, `day` columns) are left as recorded. Keys are not changed, so each loop updates the same Forge objects and Heimdall entities rather than creating new ones; this is deliberate.

---

### Task 1: Crate skeleton and time shifting for observations

**Files:**
- Create: `tools/replay/Cargo.toml`, `tools/replay/src/lib.rs`, `tools/replay/src/shift.rs`
- Modify: `Cargo.toml` (workspace members: add `"tools/replay"` after `"tools/mg-model"`)
- Test: `tools/replay/tests/shift.rs`

**Step 1: Cargo.toml**

```toml
[package]
name = "midgard-replay"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Plays a recorded hour of Heimdall feed batches and Forge source files back on a loop, times shifted to now, through the real signed ingest path; for the offline presentation app."

[[bin]]
name = "midgard-replay"
path = "src/main.rs"

[dependencies]
# Strict options and the dependency-free JSON value, as every Midgard binary uses.
service-runtime = { path = "../../services/runtime" }
```

`src/lib.rs`:

```rust
//! midgard-replay: the recording, time shifting and the loop. [design §2]
pub mod shift;
```

Create `src/main.rs` with `fn main() {}` for now.

**Step 2: Write the failing test** (`tests/shift.rs`)

```rust
use midgard_replay::shift::{is_time_key, shift_observation};
use service_runtime::json::parse;

#[test]
fn shifts_observed_at_and_time_attributes_only() {
    let o = parse(r#"{"entity":"E1","kind":"event-fire","observedAt":1792000000,
        "attrs":{"acq_time":1791990000,"issued_at":1791000000,"frp":3.5,"name":"x",
                 "valid_to":1792003600,"tiny_at":12,"lat":51.5}}"#).unwrap();
    let s = shift_observation(&o, 3600).encode();
    let v = parse(&s).unwrap();
    assert_eq!(v.get("observedAt").unwrap().as_i64(), Some(1792003600));
    let a = v.get("attrs").unwrap();
    assert_eq!(a.get("acq_time").unwrap().as_i64(), Some(1791993600));
    assert_eq!(a.get("issued_at").unwrap().as_i64(), Some(1791003600));
    assert_eq!(a.get("valid_to").unwrap().as_i64(), Some(1792007200));
    assert_eq!(a.get("tiny_at").unwrap().as_i64(), Some(12)); // not a Unix time
    assert_eq!(a.get("name").unwrap().as_str(), Some("x"));
    assert!(s.contains("\"frp\":3.5") && s.contains("\"lat\":51.5"));
}

#[test]
fn time_keys() {
    for k in ["pos_at", "event_time", "time_raised", "started", "etr", "valid_from", "published"] {
        assert!(is_time_key(k), "{k}");
    }
    for k in ["lat", "frp", "timeline", "name"] {
        assert!(!is_time_key(k), "{k}");
    }
}
```

**Step 3: Run it, expect a compile failure** (`shift` is empty):
`cargo test --offline -p midgard-replay --test shift`

**Step 4: Implement `src/shift.rs`**

```rust
//! Moving a recording's times to now. [design §2]
use service_runtime::json::Json;

/// Unix seconds between these bounds are times; anything else is left alone.
const MIN_UNIX: i64 = 1_000_000_000;
const MAX_UNIX: i64 = 4_000_000_000;
const NAMED: &[&str] = &["started", "ended", "etr", "valid_from", "valid_to", "published"];

/// Whether an attribute (or CSV column) holds a time to move.
pub fn is_time_key(key: &str) -> bool {
    key.ends_with("_at") || key.ends_with("_time") || key.starts_with("time_") || NAMED.contains(&key)
}

fn shift_value(v: &Json, shift: i64) -> Json {
    match v {
        Json::Int(n) if (MIN_UNIX..=MAX_UNIX).contains(n) => Json::Int(n + shift),
        Json::Num(f) if (MIN_UNIX as f64..=MAX_UNIX as f64).contains(f) => Json::Num(f + shift as f64),
        other => other.clone(),
    }
}

/// A copy of the observation with `observedAt` and its time attributes moved by `shift` s.
pub fn shift_observation(o: &Json, shift: i64) -> Json {
    let Json::Obj(pairs) = o else { return o.clone() };
    Json::Obj(pairs.iter().map(|(k, v)| {
        let v = match (k.as_str(), v) {
            ("observedAt", v) => shift_value(v, shift),
            ("attrs", Json::Obj(attrs)) => Json::Obj(attrs.iter().map(|(ak, av)| {
                (ak.clone(), if is_time_key(ak) { shift_value(av, shift) } else { av.clone() })
            }).collect()),
            (_, v) => v.clone(),
        };
        (k.clone(), v)
    }).collect())
}
```

**Step 5: Run the test, expect PASS.** Then `cargo build --offline -p midgard-replay` (this adds the crate to `Cargo.lock`; check `git diff Cargo.lock` only adds `midgard-replay`).

**Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock tools/replay
git commit -S -m "midgard-replay: crate and time shifting for observations"
```

### Task 2: Time shifting for Forge CSV files

**Files:** Modify `tools/replay/src/shift.rs`; Test: `tools/replay/tests/shift.rs`

**Step 1: Failing test** (append)

```rust
use midgard_replay::shift::shift_csv;

#[test]
fn shifts_time_columns_and_keeps_every_other_byte() {
    let csv = "key,message,time_raised,severity,observed_at\n\
               A,\"Rivers Bure, Ant and \"\"Thurne\"\"\",1792153093,3,\n\
               B,plain,1792000000,2,1792000100\n";
    let out = shift_csv(csv, 60);
    assert_eq!(out, "key,message,time_raised,severity,observed_at\n\
                     A,\"Rivers Bure, Ant and \"\"Thurne\"\"\",1792153153,3,\n\
                     B,plain,1792000060,2,1792000160\n");
}

#[test]
fn csv_without_time_columns_is_unchanged() {
    let csv = "key,name,intensity\nX,North,120\n";
    assert_eq!(shift_csv(csv, 999), csv);
}
```

**Step 2: Run, expect compile failure.**

**Step 3: Implement** (append to `shift.rs`)

```rust
/// Split CSV text into records of raw fields (quotes kept as written), on commas and
/// newlines outside quotes. Joining the fields back with ',' and '\n' gives the input.
fn records(text: &str) -> Vec<Vec<&str>> {
    let (mut out, mut rec, mut start, mut quoted) = (Vec::new(), Vec::new(), 0usize, false);
    let b = text.as_bytes();
    for i in 0..b.len() {
        match b[i] {
            b'"' => quoted = !quoted,
            b',' if !quoted => { rec.push(&text[start..i]); start = i + 1; }
            b'\n' if !quoted => {
                let end = if i > start && b[i - 1] == b'\r' { i - 1 } else { i };
                rec.push(&text[start..end]); out.push(std::mem::take(&mut rec)); start = i + 1;
            }
            _ => {}
        }
    }
    if start < text.len() { rec.push(&text[start..]); out.push(rec); }
    out
}

/// The CSV with whole-number Unix times in its time columns moved by `shift` seconds.
pub fn shift_csv(text: &str, shift: i64) -> String {
    let recs = records(text);
    let Some(header) = recs.first() else { return text.to_string() };
    let cols: Vec<usize> = header.iter().enumerate()
        .filter(|(_, h)| { let h = h.trim_matches('"'); h.ends_with("_at") || h.starts_with("time_") })
        .map(|(i, _)| i).collect();
    if cols.is_empty() { return text.to_string(); }
    let mut out = String::with_capacity(text.len() + 64);
    for (r, rec) in recs.iter().enumerate() {
        for (i, f) in rec.iter().enumerate() {
            if i > 0 { out.push(','); }
            match f.parse::<i64>() {
                Ok(n) if r > 0 && cols.contains(&i) && (MIN_UNIX..=MAX_UNIX).contains(&n) => out.push_str(&(n + shift).to_string()),
                _ => out.push_str(f),
            }
        }
        out.push('\n');
    }
    out
}
```

**Step 4: Run tests, expect PASS** (`cargo test --offline -p midgard-replay`).

**Step 5: Commit** — `git commit -S -m "midgard-replay: time shifting for Forge CSV files"`

### Task 3: Reading a recording

**Files:** Create `tools/replay/src/recording.rs`; modify `src/lib.rs` (`pub mod recording;`); Test: `tools/replay/tests/recording.rs`

**Step 1: Failing test** — builds a recording in a temp dir (gzip via `/usr/bin/gzip`):

```rust
use midgard_replay::recording::{Event, Recording};
use std::{fs, process::Command};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("replay-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("forge")).unwrap();
    d
}

#[test]
fn loads_batches_and_forge_files_in_offset_order() {
    let d = temp_dir("load");
    fs::write(d.join("meta.json"), r#"{"version":1,"recordedFrom":1792000000,"duration":600,"feeds":["adsb-fi","usgs-quakes"],"forgeFiles":["aircraft.csv"]}"#).unwrap();
    fs::write(d.join("heimdall.jsonl"), concat!(
        r#"{"offset":0,"feed":"usgs-quakes","observations":[{"entity":"Q1","kind":"event-earthquake","observedAt":1792000000,"attrs":{}}]}"#, "\n",
        r#"{"offset":90,"feed":"adsb-fi","observations":[{"entity":"A1","kind":"air-track","observedAt":1792000090,"attrs":{}}]}"#, "\n")).unwrap();
    assert!(Command::new("/usr/bin/gzip").arg(d.join("heimdall.jsonl")).status().unwrap().success());
    fs::write(d.join("forge/000000-aircraft.csv"), "key,observed_at\nA1,1792000000\n").unwrap();
    fs::write(d.join("forge/000045-aircraft.csv"), "key,observed_at\nA1,1792000045\n").unwrap();
    fs::write(d.join("forge/manifest.json"), r#"[{"offset":0,"file":"aircraft.csv","path":"forge/000000-aircraft.csv"},{"offset":45,"file":"aircraft.csv","path":"forge/000045-aircraft.csv"}]"#).unwrap();

    let rec = Recording::load(&d).unwrap();
    assert_eq!((rec.recorded_from, rec.duration), (1792000000, 600));
    let offsets: Vec<(u64, String)> = rec.events.iter().map(|e| match e {
        Event::Batch { offset, feed, .. } => (*offset, format!("batch {feed}")),
        Event::Forge { offset, file, .. } => (*offset, format!("forge {file}")),
    }).collect();
    assert_eq!(offsets, vec![(0, "batch usgs-quakes".into()), (0, "forge aircraft.csv".into()),
                             (45, "forge aircraft.csv".into()), (90, "batch adsb-fi".into())]);
}

#[test]
fn refuses_an_unknown_version() {
    let d = temp_dir("version");
    fs::write(d.join("meta.json"), r#"{"version":2,"recordedFrom":1,"duration":1}"#).unwrap();
    assert!(Recording::load(&d).unwrap_err().contains("version"));
}
```

**Step 2: Run, expect compile failure.**

**Step 3: Implement `recording.rs`**

```rust
//! A recording on disk (format version 1, see the phase 2 plan). [design §2]
use service_runtime::json::{parse, Json};
use std::path::{Path, PathBuf};
use std::process::Command;

pub enum Event {
    /// A Heimdall batch: the feed it came from and its observations as recorded.
    Batch { offset: u64, feed: String, observations: Vec<Json> },
    /// A Forge source file version: written as `file` at `offset`.
    Forge { offset: u64, file: String, path: PathBuf },
}

impl Event {
    pub fn offset(&self) -> u64 {
        match self { Event::Batch { offset, .. } | Event::Forge { offset, .. } => *offset }
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
        let out = Command::new("/usr/bin/gzip").arg("-dc").arg(&gz).output()
            .map_err(|e| format!("gzip: {e}"))?;
        if !out.status.success() { return Err(format!("gzip could not read {}", gz.display())); }
        return String::from_utf8(out.stdout).map_err(|_| "heimdall.jsonl.gz is not UTF-8".into());
    }
    let plain = dir.join("heimdall.jsonl");
    if plain.exists() { read(&plain) } else { Ok(String::new()) }
}

impl Recording {
    pub fn load(dir: &Path) -> Result<Recording, String> {
        let meta = parse(&read(&dir.join("meta.json"))?)?;
        if meta.get("version").and_then(Json::as_i64) != Some(1) {
            return Err("unsupported recording version (this replay reads version 1)".into());
        }
        let recorded_from = meta.get("recordedFrom").and_then(Json::as_i64).ok_or("meta.json: no recordedFrom")?;
        let duration = meta.get("duration").and_then(Json::as_i64).filter(|d| *d > 0).ok_or("meta.json: no duration")? as u64;
        let mut events = Vec::new();
        for (n, line) in heimdall_lines(dir)?.lines().enumerate() {
            if line.trim().is_empty() { continue; }
            let b = parse(line).map_err(|e| format!("heimdall batch {}: {e}", n + 1))?;
            let offset = b.get("offset").and_then(Json::as_i64).ok_or("batch without offset")? as u64;
            let feed = b.str_field("feed").ok_or("batch without feed")?.to_string();
            let observations = match b.get("observations") { Some(Json::Arr(a)) => a.clone(), _ => Vec::new() };
            events.push(Event::Batch { offset, feed, observations });
        }
        let manifest = dir.join("forge/manifest.json");
        if manifest.exists() {
            if let Json::Arr(items) = parse(&read(&manifest)?)? {
                for it in items {
                    let offset = it.get("offset").and_then(Json::as_i64).ok_or("forge entry without offset")? as u64;
                    let file = it.str_field("file").ok_or("forge entry without file")?;
                    if file.contains('/') || file.starts_with('.') { return Err(format!("forge file name {file} is not a plain name")); }
                    let path = dir.join(it.str_field("path").ok_or("forge entry without path")?);
                    events.push(Event::Forge { offset, file: file.to_string(), path });
                }
            }
        }
        // Stable: batches keep file order, then Forge entries, within an offset.
        events.sort_by_key(|e| (e.offset(), matches!(e, Event::Forge { .. })));
        Ok(Recording { recorded_from, duration, events })
    }
}
```

**Step 4: Run tests, expect PASS.**

**Step 5: Commit** — `git commit -S -m "midgard-replay: read a recording"`

### Task 4: The loop, with an injectable clock and sinks

**Files:** Create `tools/replay/src/player.rs`; modify `src/lib.rs` (`pub mod player;`); Test: `tools/replay/tests/player.rs`

The player owns timing only; sending is behind a trait so tests run instantly.

**Step 1: Failing test**

```rust
use midgard_replay::player::{Clock, Player, Sink};
use midgard_replay::recording::{Event, Recording};
use service_runtime::json::parse;
use std::cell::RefCell;

struct FakeClock(RefCell<i64>);
impl Clock for FakeClock {
    fn now(&self) -> i64 { *self.0.borrow() }
    fn sleep_until(&self, t: i64) { let mut n = self.0.borrow_mut(); if t > *n { *n = t; } }
}
#[derive(Default)]
struct Log(RefCell<Vec<String>>);
impl Sink for Log {
    fn batch(&self, feed: &str, lines: &str) -> Result<String, String> {
        self.0.borrow_mut().push(format!("{feed} {lines}")); Ok("ok".into())
    }
    fn forge(&self, file: &str, csv: &str) -> Result<(), String> {
        self.0.borrow_mut().push(format!("forge {file} {csv}")); Ok(())
    }
}

#[test]
fn plays_two_loops_with_times_shifted_to_each_loop() {
    let dir = std::env::temp_dir().join(format!("replay-player-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.csv"), "key,observed_at\nA,1000000100\n").unwrap();
    let rec = Recording { recorded_from: 1_000_000_000, duration: 100, events: vec![
        Event::Batch { offset: 10, feed: "f".into(), observations: vec![parse(r#"{"entity":"E","kind":"k","observedAt":1000000010,"attrs":{}}"#).unwrap()] },
        Event::Forge { offset: 50, file: "a.csv".into(), path: dir.join("a.csv") },
    ]};
    let clock = FakeClock(RefCell::new(2_000_000_000));
    let log = Log::default();
    Player { rec: &rec, clock: &clock, sink: &log }.run(Some(2));
    let got = log.0.borrow().clone();
    assert_eq!(got, vec![
        r#"f {"entity":"E","kind":"k","observedAt":2000000010,"attrs":{}}"#.to_string() + "\n",
        "forge a.csv key,observed_at\nA,2000000100\n".to_string(),
        r#"f {"entity":"E","kind":"k","observedAt":2000000110,"attrs":{}}"#.to_string() + "\n",
        "forge a.csv key,observed_at\nA,2000000200\n".to_string(),
    ]);
    assert_eq!(clock.now(), 2_000_000_150); // waited for the second loop's last event
}
```

**Step 2: Run, expect compile failure.**

**Step 3: Implement `player.rs`**

```rust
//! The replay loop: each event at its offset in every loop, times shifted to the loop.
use crate::recording::{Event, Recording};
use crate::shift::{shift_csv, shift_observation};

pub trait Clock {
    fn now(&self) -> i64;
    fn sleep_until(&self, t: i64);
}

/// Where events go: Heimdall batches (JSON lines) and Forge files.
pub trait Sink {
    fn batch(&self, feed: &str, lines: &str) -> Result<String, String>;
    fn forge(&self, file: &str, csv: &str) -> Result<(), String>;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
    }
    fn sleep_until(&self, t: i64) {
        let n = self.now();
        if t > n { std::thread::sleep(std::time::Duration::from_secs((t - n) as u64)); }
    }
}

pub struct Player<'a> {
    pub rec: &'a Recording,
    pub clock: &'a dyn Clock,
    pub sink: &'a dyn Sink,
}

/// One JSON log line (the run script waits for `"replay started"`).
pub fn log(message: &str, fields: &[(&str, String)]) {
    let mut s = format!("{{\"message\":\"{message}\"");
    for (k, v) in fields { s.push_str(&format!(",\"{k}\":{}", service_runtime::json::Json::s(v).encode())); }
    println!("{s}}}");
}

impl Player<'_> {
    /// Play `loops` loops (None: forever). A late event is played at once, never skipped,
    /// so a slow Mac lags rather than losing data.
    pub fn run(&self, loops: Option<u64>) {
        let start = self.clock.now();
        let mut k = 0u64;
        while loops.map_or(true, |n| k < n) {
            let loop_start = start + (k * self.rec.duration) as i64;
            let shift = loop_start - self.rec.recorded_from;
            log("loop started", &[("loop", k.to_string())]);
            for e in &self.rec.events {
                self.clock.sleep_until(loop_start + e.offset() as i64);
                match e {
                    Event::Batch { feed, observations, .. } => {
                        let mut lines = String::new();
                        for o in observations { lines.push_str(&shift_observation(o, shift).encode()); lines.push('\n'); }
                        let r = self.sink.batch(feed, &lines);
                        log("batch sent", &[("feed", feed.clone()), ("offset", e.offset().to_string()),
                            ("result", r.unwrap_or_else(|err| format!("FAILED {err}")))]);
                    }
                    Event::Forge { file, path, .. } => {
                        let r = std::fs::read_to_string(path).map_err(|err| err.to_string())
                            .and_then(|csv| self.sink.forge(file, &shift_csv(&csv, shift)));
                        if let Err(err) = r { log("forge file failed", &[("file", file.clone()), ("error", err)]); }
                    }
                }
            }
            // The next loop starts a full duration after this one, not after the last event.
            k += 1;
        }
    }
}
```

Note the test expects the last `sleep_until` to stop at the second loop's last event (`start + 100 + 50`), so `run` must not sleep to the loop end after the final loop.

**Step 4: Run tests, expect PASS.**

**Step 5: Commit** — `git commit -S -m "midgard-replay: the replay loop"`

### Task 5: Real sinks: heimdall-source and Forge files

**Files:** Create `tools/replay/src/sinks.rs`; modify `src/lib.rs` (`pub mod sinks;`); Test: `tools/replay/tests/sinks.rs`

**Step 1: Failing test** — a fake `heimdall-source` script records its arguments and input:

```rust
use midgard_replay::player::Sink;
use midgard_replay::sinks::LiveSink;
use std::{fs, os::unix::fs::PermissionsExt};

#[test]
fn pipes_batches_to_heimdall_source_and_writes_forge_files_atomically() {
    let d = std::env::temp_dir().join(format!("replay-sinks-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("forge")).unwrap();
    let fake = d.join("heimdall-source");
    fs::write(&fake, format!("#!/bin/sh\necho \"$@\" > {0}/args\ncat > {0}/stdin\necho 'batch: 1 accepted, 0 refused, 0 unreadable'\n", d.display())).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let sink = LiveSink { source_bin: fake, identity_dir: "/ids".into(), addr: "127.0.0.1:7730".into(), forge_dir: d.join("forge") };

    assert_eq!(sink.batch("adsb-fi", "{\"a\":1}\n").unwrap(), "batch: 1 accepted, 0 refused, 0 unreadable");
    assert_eq!(fs::read_to_string(d.join("args")).unwrap().trim(),
               "--identity-dir /ids --addr 127.0.0.1:7730 --service adsb-fi --batch -");
    assert_eq!(fs::read_to_string(d.join("stdin")).unwrap(), "{\"a\":1}\n");

    sink.forge("aircraft.csv", "key\nA\n").unwrap();
    assert_eq!(fs::read_to_string(d.join("forge/aircraft.csv")).unwrap(), "key\nA\n");
    assert!(sink.forge("../x.csv", "").is_err());
}
```

**Step 2: Run, expect compile failure.**

**Step 3: Implement `sinks.rs`**

```rust
//! The real sinks: Heimdall's signed ingest path and Forge's source directory.
use crate::player::Sink;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct LiveSink {
    /// `heimdall-source`, normally beside this binary.
    pub source_bin: PathBuf,
    /// The feed identities (`airports-mesh/feeds`): one per recorded feed.
    pub identity_dir: String,
    /// Heimdall's mesh address.
    pub addr: String,
    /// Where `forge-worker` reads its CSV sources (`live-feeds/data`).
    pub forge_dir: PathBuf,
}

impl Sink for LiveSink {
    fn batch(&self, feed: &str, lines: &str) -> Result<String, String> {
        let mut child = Command::new(&self.source_bin)
            .args(["--identity-dir", &self.identity_dir, "--addr", &self.addr, "--service", feed, "--batch", "-"])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().map_err(|e| format!("{}: {e}", self.source_bin.display()))?;
        child.stdin.take().ok_or("no stdin")?.write_all(lines.as_bytes()).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(if out.stdout.is_empty() { &out.stderr } else { &out.stdout }).trim().to_string();
        let last = text.lines().last().unwrap_or("").to_string();
        // Exit 1 with a summary means some observations were refused (a late duplicate):
        // report it, it is not a failure of the replay.
        if out.status.success() || last.starts_with("batch:") { Ok(last) } else { Err(last) }
    }

    fn forge(&self, file: &str, csv: &str) -> Result<(), String> {
        if file.contains('/') || file.starts_with('.') { return Err(format!("{file} is not a plain file name")); }
        let tmp = self.forge_dir.join(format!(".{file}.replay"));
        std::fs::write(&tmp, csv).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.forge_dir.join(file)).map_err(|e| e.to_string())
    }
}
```

**Step 4: Run tests, expect PASS.**

**Step 5: Commit** — `git commit -S -m "midgard-replay: heimdall-source and Forge file sinks"`

### Task 6: The binary

**Files:** Modify `tools/replay/src/main.rs`; Test: `tools/replay/tests/cli.rs`

CLI:

```text
midgard-replay --recording DIR --identity-dir DIR --forge-dir DIR
               [--addr HOST:PORT (default 127.0.0.1:7730)] [--source-bin PATH] [--loops N]
```

`--source-bin` defaults to `heimdall-source` beside the running binary. Before playing, it checks every recorded feed has `<feed>.cert` in the identity dir and exits 2 naming any missing ones.

**Step 1: Failing test** — runs the built binary (`env!("CARGO_BIN_EXE_midgard-replay")`) with a one-batch recording whose offsets are 0, the fake script from Task 5 and `--loops 1`; asserts exit 0, `"replay started"` in stdout, the fake script's stdin contains the shifted `observedAt` (greater than the recorded one by at least the current time minus `recordedFrom`), and a second test asserts exit 2 and `missing identity for feed nofeed` when the identity dir lacks `nofeed.cert`.

**Step 2: Run, expect FAIL.**

**Step 3: Implement `main.rs`**

```rust
//! `midgard-replay` — plays a recording back on a loop. [design §2]
use midgard_replay::player::{log, Player, SystemClock};
use midgard_replay::recording::{Event, Recording};
use midgard_replay::sinks::LiveSink;
use service_runtime::Args;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: midgard-replay --recording DIR --identity-dir DIR --forge-dir DIR \
                     [--addr HOST:PORT] [--source-bin PATH] [--loops N]";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = Args::parse(&argv, &["recording", "identity-dir", "forge-dir", "addr", "source-bin", "loops"], &[])
        .unwrap_or_else(|e| { eprintln!("midgard-replay: {e}\n{USAGE}"); std::process::exit(2) });
    let need = |k: &str| args.require(k).map(str::to_string).unwrap_or_else(|e| { eprintln!("midgard-replay: {e}\n{USAGE}"); std::process::exit(2) });
    let (rec_dir, ids, forge_dir) = (need("recording"), need("identity-dir"), need("forge-dir"));
    let loops = args.get("loops").map(|n| n.parse::<u64>().unwrap_or_else(|_| { eprintln!("midgard-replay: --loops must be a number"); std::process::exit(2) }));
    let source_bin = args.get("source-bin").map(PathBuf::from).unwrap_or_else(|| {
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("heimdall-source"))).unwrap_or_else(|| "heimdall-source".into())
    });
    let rec = Recording::load(Path::new(&rec_dir)).unwrap_or_else(|e| { eprintln!("midgard-replay: {e}"); std::process::exit(2) });
    let mut missing: Vec<&str> = rec.events.iter().filter_map(|e| match e { Event::Batch { feed, .. } => Some(feed.as_str()), _ => None })
        .filter(|f| !Path::new(&ids).join(format!("{f}.cert")).exists()).collect();
    missing.sort(); missing.dedup();
    if !missing.is_empty() { eprintln!("midgard-replay: missing identity for feed {}", missing.join(", ")); std::process::exit(2); }
    let sink = LiveSink { source_bin, identity_dir: ids, addr: args.get("addr").unwrap_or("127.0.0.1:7730").to_string(), forge_dir: forge_dir.into() };
    log("replay started", &[("events", rec.events.len().to_string()), ("duration", rec.duration.to_string())]);
    Player { rec: &rec, clock: &SystemClock, sink: &sink }.run(loops);
}
```

**Step 4: Run all crate tests, expect PASS.** Also `cargo clippy --offline -p midgard-replay -- -D warnings` and `cargo fmt -p midgard-replay`.

**Step 5: Commit** — `git commit -S -m "midgard-replay: the command line"`

### Task 7: The recorder

**Files:** Create `tools/mac-app/record.py`; Test: `tools/mac-app/test_record.py` (unittest, standard library only)

```text
python3 tools/mac-app/record.py record --minutes 60 [--name NAME]   # capture, then pack
python3 tools/mac-app/record.py pack RAW_DIR OUT_DIR                 # re-pack a capture
```

Behaviour:
- **record:** paths from `MIDGARD_HOME` (default `~/Desktop/Midgard`); output under `~/Library/Application Support/Midgard-dev/recordings/<name>` (default `rec-YYYYmmdd-HHMM`), raw capture in `<name>/raw`.
  1. Copy every `live-feeds/data/*.csv` to `raw/forge/000000-<file>` and `jobs.json` to `raw/forge/jobs.json`.
  1b. Write `raw/meta.json` with `recordedFrom` and `duration` 1 at the start (updated at the end), so `pack` also works on a capture that crashed.
  2. Create `raw/incoming/` and write `"<its absolute path>\npid=<own pid>"` into `heimdall-poc/feeds/state/record_to` (the feeds.py hook). SIGINT, SIGTERM and SIGHUP all set the same stopping flag (previous handlers restored afterwards), so Ctrl-C, a kill or a closed terminal still packs. Then stop the feeds runner (`heimdall-poc/feeds/stop.sh`), delete `heimdall-poc/feeds/state/last_sent/*.json`, start it (`start.sh`), so every source sends everything once. The scripts run in their own session (Ctrl-C does not reach them; a Ctrl-C during the restart takes effect after it). Check both exit codes; 3 s after `start.sh` the pid in `feeds.pid` must be alive, otherwise remove `record_to` and the `<name>` folder (so the name can be reused), print "feeds runner did not start: run heimdall-poc/feeds/start.sh" and exit non-zero without packing.
  3. Every 0.5 s until the time is up: move every complete file (not a dot-file) from `raw/incoming/` to `raw/heimdall/<offset:06d>-<feed>.jsonl` (offset = time_ns/1e9 - recordedFrom, clamped to 0; a second batch of a feed in the same second gets `~002`, `~003`...). Poll `heimdall-poc/batch.jsonl` (feed `adsb-lol`) and `live-feeds/data/*.csv`: when the mtime is newer than last copied (the CSVs' first version is the offset-0 copy) and has not changed for 1 s, copy it to `raw/heimdall/<offset:06d>-adsb-lol.jsonl` or `raw/forge/<offset:06d>-<file>`; skip a batch file whose lines do not all parse as JSON (mid-write; retried next poll). `heimdall-poc/feeds/out/` is not polled. Progress every minute, with a warning if no Heimdall batch has arrived after 2 minutes. `record_to` is always removed when capture ends (also on Ctrl-C and errors), then the last incoming files are moved.
  4. Write `raw/meta.json` (`recordedFrom`, `duration`), then pack into `<name>`. `--drop-raw` deletes `raw/` after a successful pack (default: kept).
- **pack (pure functions, tested):**
  - `thin(batches)`: for kinds `air-track`, `sea-vessel`, `air-balloon`, `land-train`, keep an entity's observation only if 30 s or more after the last one kept (by `observedAt`). Keyed on (kind, entity) across feeds, so adsb-fi's and adsb-lol's reports of one aircraft are thinned together.
  - Observations containing NaN or Infinity are dropped and counted per feed; batches are written with `allow_nan=False`. Forge files are hard-linked from `raw/` (copied if linking fails).
  - Drop kinds `land-camera` and `land-camera-ai` (images need the internet).
  - Drop empty batches; write `heimdall.jsonl.gz` (gzip module), `forge/manifest.json`, the forge files, `forge/jobs.json`, `meta.json` (`version` 1, `feeds` sorted, `forgeFiles` sorted).
  - Print the packed and raw sizes and the number of batches and observations per feed.

**Step 1: Failing tests** in `test_record.py`: `thin` keeps 0 s and 30 s and drops 10 s for one aircraft but keeps all of an earthquake's; cameras dropped; `pack` on a temp raw dir writes a `meta.json` with `version` 1 and a `heimdall.jsonl.gz` whose lines are in offset order and that `midgard-replay`'s format expects (`offset`, `feed`, `observations`).

**Step 2:** `python3 -m unittest tools/mac-app/test_record.py` → FAIL.

**Step 3:** Implement `record.py` (standard library only; `subprocess.run([...stop.sh])` etc.; `signal` handling so Ctrl-C packs what was captured).

**Step 4:** Tests PASS.

**Step 5: Commit** — `git commit -S -m "mac-app: recorder for an hour of feed batches and Forge files"`

### Task 8: Build and run scripts, then end-to-end check

**Files:** Modify `tools/mac-app/build-services.sh`, `tools/mac-app/run-native.sh`, `tools/mac-app/README.md`

1. `build-services.sh`: add `-p midgard-replay` to `PACKAGES` and `midgard-replay` to `BINARIES`.
2. `run-native.sh start`: when `MIDGARD_REPLAY` is set (a recording directory), after forge is ready, `launch replay '"replay started"' "$BIN/midgard-replay" --recording "$MIDGARD_REPLAY" --identity-dir "$H/airports-mesh/feeds" --forge-dir "$H/live-feeds/data"`. Add `replay` to `stop` (first) and `status`. It has no lock file; `lock_path replay` returns nothing and the stale-lock sweep skips it.
3. README: a "Recording and replay" section: how to record (live Docker suite and feeds running), where recordings are kept, how to run native with `MIDGARD_REPLAY`, storage note.
4. **End-to-end (manual, report results):**
   - With the Docker suite and feeds running: `python3 tools/mac-app/record.py record --minutes 5 --name e2e`. Expect a summary with 30 or more feeds and Forge files.
   - Stop feeds and containers as README "To run natively" says; `bash tools/mac-app/build-services.sh`; `MIDGARD_REPLAY="$HOME/Library/Application Support/Midgard-dev/recordings/e2e" bash tools/mac-app/run-native.sh start`.
   - Expect: `native-run/replay.log` shows `batch sent` lines with `accepted=` results and no `FAILED`; `python3 tools/mac-app/smoke.py` passes; Heimdall's picture has aircraft whose `lastSeen` is within 2 minutes of now; Forge's overview shows jobs run after the replay started.
   - Let it pass the 5-minute seam once: the second loop's batches are accepted (no duplicate refusals).
   - `run-native.sh stop`, restart the Docker suite and feeds (README "To go back to Docker"), delete the build cache (`rm -rf ~/Library/Caches/midgard-build`) and check `du -sh ~/Desktop/Midgard` is within 3.5 GB.
5. **Commit** — `git commit -S -m "mac-app: run the replay with the native suite"` and push.

The full one-hour recording is made in phase 4, when the hour is chosen (power cuts, weather warnings and flood alerts present) and the prepared Mimir answers are recorded against it.
