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
    /// Pipe the batch to `heimdall-source` and return its summary line.
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
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        // Written from its own thread while the output is read, so neither side can
        // block the other; dropped at the end so the child sees EOF. A write error (the
        // child exited without reading it all) is left to the child's own message.
        let out = std::thread::scope(|s| {
            s.spawn(move || {
                let _ = stdin.write_all(lines.as_bytes());
            });
            child.wait_with_output()
        })
        .map_err(|e| format!("{bin}: {e}"))?;
        let text = String::from_utf8_lossy(if out.stdout.is_empty() {
            &out.stderr
        } else {
            &out.stdout
        })
        .trim()
        .to_string();
        let last = text.lines().last().unwrap_or("").to_string();
        // Exit 1 with a summary means some observations were refused (a late duplicate):
        // report it, it is not a failure of the replay.
        if out.status.success() || last.starts_with("batch:") {
            Ok(last)
        } else if last.is_empty() {
            Err(format!("{bin} failed ({})", out.status))
        } else {
            Err(last)
        }
    }

    /// Replace `file` atomically: written beside it, then renamed over it, so
    /// `forge-worker` never reads half a file.
    fn forge(&self, file: &str, csv: &str) -> Result<(), String> {
        if file.is_empty() || file.contains('/') || file.contains('\\') || file.starts_with('.') {
            return Err(format!("{file} is not a plain file name"));
        }
        let tmp = self.forge_dir.join(format!(".{file}.replay"));
        std::fs::write(&tmp, csv).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let dest = self.forge_dir.join(file);
        std::fs::rename(&tmp, &dest).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", dest.display())
        })
    }
}
