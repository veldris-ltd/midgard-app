//! The real sinks: Heimdall's signed ingest path and Forge's source directory.
use crate::player::Sink;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct LiveSink {
    /// `heimdall-source`, normally beside this binary.
    pub source_bin: PathBuf,
    /// The feed identities (`airports-mesh/feeds`): one per recorded feed.
    pub identity_dir: String,
    /// Heimdall's mesh address.
    pub addr: String,
    /// Where `forge-worker` reads its CSV sources (`live-feeds/data`).
    pub forge_dir: PathBuf,
    /// How long one `heimdall-source` run may take before it is killed.
    pub timeout: Duration,
}

/// The `heimdall-source` the replay runs when `--source-bin` is not given: the one beside
/// `exe` (this binary), named with the platform's executable suffix (`.exe` on Windows,
/// none on Unix), or the bare name for PATH when `exe` has no directory.
pub fn default_source_bin(exe: Option<&Path>) -> PathBuf {
    let name = format!("heimdall-source{}", std::env::consts::EXE_SUFFIX);
    exe.and_then(Path::parent)
        .map(|d| d.join(&name))
        .unwrap_or_else(|| name.into())
}

/// How many times a Forge file's rename-over is tried while it is refused for a reason
/// that passes, and the waits between tries: from `RENAME_FIRST_WAIT`, doubling, at
/// most `RENAME_MAX_WAIT` (about 3 s in all).
pub const RENAME_TRIES: u32 = 10;
pub const RENAME_FIRST_WAIT: Duration = Duration::from_millis(50);
pub const RENAME_MAX_WAIT: Duration = Duration::from_millis(500);

/// Whether a refused rename-over is worth trying again. On Windows `forge-worker` may
/// have the CSV open while it reads it, and the rename (MoveFileExW) is then refused
/// with a sharing violation (ERROR_SHARING_VIOLATION, 32) or access denied
/// (ERROR_ACCESS_DENIED, 5) until it closes it. Never on Unix, where a rename over an
/// open file succeeds and any failure is final.
pub fn rename_refusal_passes(e: &io::Error) -> bool {
    cfg!(windows) && matches!(e.raw_os_error(), Some(5) | Some(32))
}

/// Call `rename` until it succeeds or fails for a reason `passes` does not accept,
/// at most `RENAME_TRIES` times, calling `sleep` with the backoff between tries; the
/// last error is returned.
pub fn rename_with_retry(
    mut rename: impl FnMut() -> io::Result<()>,
    passes: impl Fn(&io::Error) -> bool,
    mut sleep: impl FnMut(Duration),
) -> io::Result<()> {
    let mut wait = RENAME_FIRST_WAIT;
    for _ in 1..RENAME_TRIES {
        match rename() {
            Err(e) if passes(&e) => {
                sleep(wait);
                wait = (wait * 2).min(RENAME_MAX_WAIT);
            }
            done => return done,
        }
    }
    rename()
}

/// How often a running child is checked against its deadline.
const POLL: Duration = Duration::from_millis(50);

/// The last non-empty line of `bytes`, trimmed.
fn last_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Read all of `r` on its own thread. Not scoped: a child killed for its deadline may
/// leave a descendant holding the pipe, and the replay must not wait for that.
fn drain(r: Option<impl Read + Send + 'static>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut r) = r {
            let _ = r.read_to_end(&mut buf);
        }
        buf
    })
}

/// How many `heimdall-source` runs `batches` keeps going at once.
pub const SIDE_BY_SIDE: usize = 4;

impl Sink for LiveSink {
    /// Up to `SIDE_BY_SIDE` batches at a time (each its own `heimdall-source` run), the
    /// results in the order given.
    fn batches(&self, items: &[(String, String)]) -> Vec<Result<String, String>> {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let results: Vec<std::sync::Mutex<Option<Result<String, String>>>> =
            items.iter().map(|_| std::sync::Mutex::new(None)).collect();
        std::thread::scope(|scope| {
            for _ in 0..SIDE_BY_SIDE.min(items.len()) {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((feed, lines)) = items.get(i) else {
                        break;
                    };
                    let r = self.batch(feed, lines);
                    if let Ok(mut slot) = results[i].lock() {
                        *slot = Some(r);
                    }
                });
            }
        });
        results
            .into_iter()
            .map(|m| {
                m.into_inner()
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| Err("not sent".to_string()))
            })
            .collect()
    }

    /// Pipe the batch to `heimdall-source` and return its summary line; a child still
    /// running after `timeout` is killed.
    fn batch(&self, feed: &str, lines: &str) -> Result<String, String> {
        let bin = self.source_bin.display();
        let mut child = Command::new(&self.source_bin)
            .args([
                "--identity-dir",
                &self.identity_dir,
                "--addr",
                &self.addr,
                "--service",
                feed,
                "--batch",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{bin}: {e}"))?;
        // Input and output on their own threads, so neither side can block the other;
        // stdin is closed once written so the child sees EOF. A write error (the child
        // exited without reading it all) is left to the child's own message.
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let input = lines.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait().map_err(|e| format!("{bin}: {e}"))? {
                Some(status) => break status,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{bin} timed out after {} s",
                        self.timeout.as_secs()
                    ));
                }
                None => std::thread::sleep(POLL),
            }
        };
        let stdout = last_line(&stdout.join().unwrap_or_default());
        let stderr = last_line(&stderr.join().unwrap_or_default());
        // Exit 1 with a summary means some observations were refused (a late duplicate):
        // report it, it is not a failure of the replay.
        if status.success() || stdout.starts_with("batch:") {
            Ok(stdout)
        } else if !stderr.is_empty() {
            Err(stderr)
        } else if !stdout.is_empty() {
            Err(stdout)
        } else {
            Err(format!("{bin} failed ({status})"))
        }
    }

    /// Replace `file` atomically: written beside it, then renamed over it, so
    /// `forge-worker` never reads half a file. No fsync before the rename: a crash loses
    /// at most this one version, which the next loop writes again. On Windows a rename
    /// refused while `forge-worker` has the file open is tried again for a few seconds
    /// (`rename_with_retry`); on Unix it is tried once.
    fn forge(&self, file: &str, csv: &str) -> Result<(), String> {
        if file.is_empty() || file.contains('/') || file.contains('\\') || file.starts_with('.') {
            return Err(format!("{file} is not a plain file name"));
        }
        let tmp = self.forge_dir.join(format!(".{file}.replay"));
        std::fs::write(&tmp, csv).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let dest = self.forge_dir.join(file);
        rename_with_retry(
            || std::fs::rename(&tmp, &dest),
            rename_refusal_passes,
            std::thread::sleep,
        )
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", dest.display())
        })
    }
}
