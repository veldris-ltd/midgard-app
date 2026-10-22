//! The replay loop: each event at its offset in every loop, times shifted to the loop.
use crate::recording::{Event, Recording};
use crate::shift::{shift_csv, shift_observation};
use service_runtime::json::Json;

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
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
    fn sleep_until(&self, t: i64) {
        let n = self.now();
        if t > n {
            std::thread::sleep(std::time::Duration::from_secs((t - n) as u64));
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
pub fn log(message: &str, fields: &[(&str, String)]) {
    println!("{}", log_line(message, fields));
}

impl Player<'_> {
    /// Play `loops` loops (None: forever). A late event is played at once, never skipped,
    /// so a slow Mac lags rather than losing data. Each loop waits for its own start, so
    /// an empty recording sleeps a loop at a time instead of spinning.
    pub fn run(&self, loops: Option<u64>) {
        let start = self.clock.now();
        let mut k = 0u64;
        while loops.is_none_or(|n| k < n) {
            let loop_start = start + (k * self.rec.duration) as i64;
            self.clock.sleep_until(loop_start);
            let shift = loop_start - self.rec.recorded_from;
            log("loop started", &[("loop", k.to_string())]);
            for e in &self.rec.events {
                self.clock.sleep_until(loop_start + e.offset() as i64);
                self.play(e, shift);
            }
            k += 1;
        }
    }

    fn play(&self, e: &Event, shift: i64) {
        match e {
            Event::Batch {
                feed, observations, ..
            } => {
                let mut lines = String::new();
                for o in observations {
                    lines.push_str(&shift_observation(o, shift).encode());
                    lines.push('\n');
                }
                let r = self.sink.batch(feed, &lines);
                log(
                    "batch sent",
                    &[
                        ("feed", feed.clone()),
                        ("offset", e.offset().to_string()),
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
