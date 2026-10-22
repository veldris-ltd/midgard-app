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
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", log_line(message, fields));
    let _ = out.flush();
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
                self.play(e, shift, lag);
            }
            loop_start = next;
            k += 1;
        }
    }

    fn play(&self, e: &Event, shift: i64, lag: i64) {
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
                log(
                    "batch sent",
                    &[
                        ("feed", feed.clone()),
                        ("offset", e.offset().to_string()),
                        ("lag", lag.to_string()),
                        ("result", r.unwrap_or_else(|err| format!("FAILED {err}"))),
                    ],
                );
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
