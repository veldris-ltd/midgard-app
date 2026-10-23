//! The replay loop: each event at its offset in every loop, times shifted to the loop.
use crate::recording::{Event, Recording};
use crate::shift::{shift_csv, shift_observation};
use service_runtime::json::{parse, Json};

/// How late (s) an event may be and still be played: past this (the Mac slept, or a
/// sink stalled) the rest of the loop is dropped and the next loop starts from now.
pub const MAX_LAG: i64 = 300;

pub trait Clock {
    fn now(&self) -> i64;
    fn sleep_until(&self, t: i64);
}

/// Where events go: Heimdall batches (JSON lines) and Forge files.
pub trait Sink {
    fn batch(&self, feed: &str, lines: &str) -> Result<String, String>;
    fn forge(&self, file: &str, csv: &str) -> Result<(), String>;
    /// Several batches (feed, lines), one result each in order. One at a time unless the
    /// sink can send them side by side.
    fn batches(&self, items: &[(String, String)]) -> Vec<Result<String, String>> {
        items.iter().map(|(f, l)| self.batch(f, l)).collect()
    }
}

/// The longest single sleep: how late a wait can end after the Mac wakes.
const SLEEP_PIECE: std::time::Duration = std::time::Duration::from_secs(5);

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
    /// Sleep to the exact instant `t`, not to the whole second after it: in pieces of
    /// at most `SLEEP_PIECE`, the wall clock read again after each, since a sleeping
    /// Mac may not count towards `thread::sleep` and a wait spanning a lid close must
    /// end as soon as it wakes.
    fn sleep_until(&self, t: i64) {
        let Ok(t) = u64::try_from(t) else { return };
        let target = std::time::UNIX_EPOCH + std::time::Duration::from_secs(t);
        while let Ok(left) = target.duration_since(std::time::SystemTime::now()) {
            if left.is_zero() {
                return;
            }
            std::thread::sleep(left.min(SLEEP_PIECE));
        }
    }
}

pub struct Player<'a> {
    pub rec: &'a Recording,
    pub clock: &'a dyn Clock,
    pub sink: &'a dyn Sink,
}

/// One JSON log line, every string escaped, no newline inside.
pub fn log_line(message: &str, fields: &[(&str, String)]) -> String {
    let mut pairs = vec![("message".to_string(), Json::s(message))];
    pairs.extend(fields.iter().map(|(k, v)| (k.to_string(), Json::s(v))));
    Json::Obj(pairs).encode()
}

/// Print one JSON log line (the run script waits for `"replay started"`).
/// Flushed at once, so a reader of a redirected file sees each line as it is logged.
pub fn log(message: &str, fields: &[(&str, String)]) {
    use std::io::Write;
    // println! rather than a locked write, so test runs capture it.
    println!("{}", log_line(message, fields));
    let _ = std::io::stdout().flush();
}

/// The most observations sent in one `heimdall-source` run: a busy minute of aircraft is
/// tens of thousands, and one huge submission is slow to sign and easy to time out.
pub const CHUNK_LINES: usize = 2000;

/// The counts in a `batch: N accepted, M refused, K unreadable` summary.
fn counts(summary: &str) -> Option<[u64; 3]> {
    let parts: Vec<&str> = summary.strip_prefix("batch: ")?.split(", ").collect();
    if parts.len() != 3 {
        return None;
    }
    let mut out = [0; 3];
    for (part, (slot, word)) in
        parts
            .into_iter()
            .zip(out.iter_mut().zip(["accepted", "refused", "unreadable"]))
    {
        let mut words = part.split_whitespace();
        *slot = words.next()?.parse().ok()?;
        if words.next()? != word {
            return None;
        }
    }
    Some(out)
}

/// One result for a batch sent in chunks: the first error if any chunk failed; else the
/// summaries' counts added up (a single chunk's summary as it was), or, if one is not a
/// summary, every chunk's text.
pub fn combine(results: Vec<Result<String, String>>) -> Result<String, String> {
    let oks = results.into_iter().collect::<Result<Vec<_>, _>>()?;
    if oks.len() == 1 {
        return Ok(oks.into_iter().next().unwrap_or_default());
    }
    let parsed: Option<Vec<[u64; 3]>> = oks.iter().map(|s| counts(s)).collect();
    match parsed {
        Some(all) => {
            let sum = |i: usize| all.iter().map(|c| c[i]).sum::<u64>();
            Ok(format!(
                "batch: {} accepted, {} refused, {} unreadable in {} chunks",
                sum(0),
                sum(1),
                sum(2),
                all.len()
            ))
        }
        None => Ok(oks.join("; ")),
    }
}

/// How often (s) a run of one repeated batch error is reported.
pub const FAILING_EVERY: i64 = 60;

/// Log lines for batch results, keeping a run of identical failures (Heimdall down for
/// minutes) from flooding the log: the first is logged in full, repeats are counted and
/// reported at most once per `FAILING_EVERY` s, and the first success after says so.
#[derive(Default)]
pub struct Failures {
    /// The error of the last failed batch, while failing.
    error: Option<String>,
    /// Batches failed since the last success.
    failed: u64,
    /// Repeats not logged since the last line about them.
    dropped: u64,
    /// When the repeated error was last logged.
    reported: i64,
}

/// A log line: its message and fields.
pub type Line = (&'static str, Vec<(&'static str, String)>);

impl Failures {
    /// The lines to log for one batch result at `now`; `fields` describe the batch.
    pub fn note(
        &mut self,
        now: i64,
        fields: &[(&'static str, String)],
        result: Result<&str, &str>,
    ) -> Vec<Line> {
        let mut out = Vec::new();
        let sent = |text: String| {
            let mut f = fields.to_vec();
            f.push(("result", text));
            ("batch sent", f)
        };
        match result {
            Ok(summary) => {
                if self.error.take().is_some() {
                    out.push((
                        "batches recovered",
                        vec![
                            ("failed", self.failed.to_string()),
                            ("dropped", self.dropped.to_string()),
                        ],
                    ));
                }
                self.failed = 0;
                self.dropped = 0;
                out.push(sent(summary.to_string()));
            }
            Err(err) => {
                self.failed += 1;
                if self.error.as_deref() == Some(err) {
                    self.dropped += 1;
                    if now - self.reported >= FAILING_EVERY {
                        out.push(self.failing(err));
                        self.reported = now;
                    }
                } else {
                    if let Some(old) = self.error.take() {
                        if self.dropped > 0 {
                            out.push(self.failing(&old));
                        }
                    }
                    out.push(sent(format!("FAILED {err}")));
                    self.error = Some(err.to_string());
                    self.reported = now;
                }
            }
        }
        out
    }

    /// A "batches failing" line for the repeats of `err` dropped so far, resetting them.
    fn failing(&mut self, err: &str) -> Line {
        let dropped = std::mem::take(&mut self.dropped);
        (
            "batches failing",
            vec![("error", err.to_string()), ("dropped", dropped.to_string())],
        )
    }
}

impl Player<'_> {
    /// Play `loops` loops (None: forever). A late event is played at once, never skipped,
    /// so a slow Mac lags rather than losing data; but once more than `MAX_LAG` behind,
    /// the rest of the loop is dropped and the next loop starts from now (re-anchored, its
    /// shift from that start). Each loop waits for its own start, so a loop whose events
    /// all come early still lasts the recording's duration (a recording always has at
    /// least one event: `Recording::load` refuses an empty one).
    pub fn run(&self, loops: Option<u64>) {
        let mut loop_start = self.clock.now();
        let mut k = 0u64;
        let mut failures = Failures::default();
        while loops.is_none_or(|n| k < n) {
            self.clock.sleep_until(loop_start);
            let shift = loop_start - self.rec.recorded_from;
            log("loop started", &[("loop", k.to_string())]);
            let mut next = loop_start + self.rec.duration as i64;
            let primed = if k == 0 {
                self.prime(&mut failures)
            } else {
                Vec::new()
            };
            for (i, e) in self.rec.events.iter().enumerate() {
                if primed.contains(&i) {
                    continue;
                }
                let due = loop_start + e.offset() as i64;
                self.clock.sleep_until(due);
                let now = self.clock.now();
                let lag = now - due;
                if lag > MAX_LAG {
                    log(
                        "behind; restarting the loop",
                        &[("loop", k.to_string()), ("lag", lag.to_string())],
                    );
                    next = now;
                    break;
                }
                self.play(e, shift, lag, &mut failures);
            }
            loop_start = next;
            k += 1;
        }
    }

    /// Primes a fresh start: when the first loop starts, every feed's first batch is sent
    /// at once rather than as each feed first reported in the hour, so every layer shows
    /// within seconds. Times are shifted to now; all go through `Sink::batches` (side by
    /// side where the sink can), smallest first. Returns their event indices, to skip in
    /// this loop.
    fn prime(&self, failures: &mut Failures) -> Vec<usize> {
        let mut seen = std::collections::BTreeSet::new();
        let mut firsts: Vec<(usize, usize)> = Vec::new();
        for (i, e) in self.rec.events.iter().enumerate() {
            if let Event::Batch { feed, lines, .. } = e {
                if seen.insert(feed.as_str()) {
                    firsts.push((lines.len(), i));
                }
            }
        }
        firsts.sort_by_key(|(size, _)| *size);
        let now = self.clock.now();
        let mut items = Vec::new();
        let mut spans = Vec::new();
        for (_, i) in &firsts {
            if let Event::Batch {
                feed,
                lines,
                offset,
            } = &self.rec.events[*i]
            {
                let shift = now - self.rec.recorded_from - *offset as i64;
                let chunks = chunks_of(lines, shift);
                spans.push((*i, items.len(), chunks.len()));
                items.extend(chunks.into_iter().map(|c| (feed.clone(), c)));
            }
        }
        let mut results = self.sink.batches(&items).into_iter();
        for (i, _, n) in &spans {
            let r = combine(results.by_ref().take(*n).collect());
            self.report(&self.rec.events[*i], 0, r, failures);
        }
        firsts.into_iter().map(|(_, i)| i).collect()
    }

    fn play(&self, e: &Event, shift: i64, lag: i64, failures: &mut Failures) {
        match e {
            Event::Batch { feed, lines, .. } => {
                // Sent in chunks of at most CHUNK_LINES; every chunk is tried even after
                // one fails, and the batch is logged once with the combined result.
                let results = chunks_of(lines, shift)
                    .iter()
                    .map(|chunk| self.sink.batch(feed, chunk))
                    .collect();
                self.report(e, lag, combine(results), failures);
            }
            Event::Forge { file, path, .. } => {
                let r = std::fs::read_to_string(path)
                    .map_err(|err| format!("{}: {err}", path.display()))
                    .and_then(|csv| self.sink.forge(file, &shift_csv(&csv, shift)));
                if let Err(err) = r {
                    log(
                        "forge file failed",
                        &[("file", file.clone()), ("error", err)],
                    );
                }
            }
        }
    }

    /// Log one batch's combined result (repeated failures folded, see `Failures`).
    fn report(&self, e: &Event, lag: i64, r: Result<String, String>, failures: &mut Failures) {
        let feed = match e {
            Event::Batch { feed, .. } => feed.clone(),
            Event::Forge { file, .. } => file.clone(),
        };
        let fields = [
            ("feed", feed),
            ("offset", e.offset().to_string()),
            ("lag", lag.to_string()),
        ];
        let lines = failures.note(
            self.clock.now(),
            &fields,
            r.as_deref().map_err(String::as_str),
        );
        for (message, fields) in lines {
            log(message, &fields);
        }
    }
}

/// A batch's observations with their times moved by `shift`, in chunks of at most
/// `CHUNK_LINES` lines, each line ending in a newline.
fn chunks_of(lines: &str, shift: i64) -> Vec<String> {
    let all: Vec<&str> = lines.lines().collect();
    all.chunks(CHUNK_LINES)
        .map(|chunk| {
            let mut out = String::new();
            for line in chunk {
                // Validated as objects at load; one that fails is sent unchanged.
                match parse(line) {
                    Ok(o) => out.push_str(&shift_observation(&o, shift).encode()),
                    Err(_) => out.push_str(line),
                }
                out.push('\n');
            }
            out
        })
        .collect()
}
