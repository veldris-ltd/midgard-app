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
            for e in &self.rec.events {
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

    fn play(&self, e: &Event, shift: i64, lag: i64, failures: &mut Failures) {
        match e {
            Event::Batch { feed, lines, .. } => {
                let mut out = String::with_capacity(lines.len() + 64);
                for line in lines.lines() {
                    // Validated as objects at load; a line that fails is sent unchanged.
                    match parse(line) {
                        Ok(o) => out.push_str(&shift_observation(&o, shift).encode()),
                        Err(_) => out.push_str(line),
                    }
                    out.push('\n');
                }
                let r = self.sink.batch(feed, &out);
                let fields = [
                    ("feed", feed.clone()),
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
}
