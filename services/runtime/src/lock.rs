//! Exclusive use of a directory of append-only journals, released when dropped.
//! [RC-09] [RC-10]
//!
//! Journals, ledgers and stores are written by one process at a time; a second writer
//! would interleave frames and the next open would fail closed. The gateway and
//! `mg-admin` hold an enclosure's state directory this way, and each mesh service holds
//! its own.
//!
//! A process killed outright (SIGKILL, an OOM kill, a container stop) never runs its
//! drop and leaves the lock file behind. The lock records its holder's pid and, on
//! Linux, that process's start time, so a later acquire can tell a live holder from a
//! dead one — including a new process that happens to reuse the pid, as a restarted
//! container's pid 1 does — and takes over only a lock whose holder is gone. Where the
//! start time cannot be read, a leftover lock is refused and an operator removes it
//! after confirming nothing is running.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// A held lock on a directory.
#[derive(Debug)]
pub struct DirLock {
    path: PathBuf,
}

impl DirLock {
    /// Take the lock file `name` in `dir`, or explain who holds it.
    pub fn acquire(dir: &Path, name: &str, holder: &str) -> Result<Self, String> {
        let path = dir.join(name);
        match Self::create(&path, holder) {
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let existing = std::fs::read_to_string(&path).unwrap_or_default();
                if !holder_is_gone(&existing) {
                    return Err(format!(
                        "{} is in use by {}; if nothing is running, remove {}",
                        dir.display(),
                        existing.lines().next().unwrap_or("another process"),
                        path.display()
                    ));
                }
                // The holder is dead: take the lock over, once. If another process
                // takes it in between, `create_new` refuses and so do we.
                std::fs::remove_file(&path)
                    .and_then(|()| Self::create(&path, holder))
                    .map_err(|e| format!("cannot take over the stale lock {}: {e}", path.display()))
            }
            Err(e) => Err(format!("cannot lock {}: {e}", dir.display())),
            ok => ok.map_err(|e| e.to_string()),
        }
    }

    fn create(path: &Path, holder: &str) -> std::io::Result<Self> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let pid = std::process::id();
        let start = process_start(pid).map_or_else(|| "unknown".to_string(), |s| s.to_string());
        writeln!(file, "{holder}\npid={pid}\nstart={start}")?;
        file.sync_all()?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

/// Whether the process a lock file names has provably exited. Anything unreadable or
/// unrecognised counts as still running.
fn holder_is_gone(lock: &str) -> bool {
    let field = |key: &str| {
        lock.lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    match (field("pid="), field("start=")) {
        (Some(pid), Some(start)) => match u32::try_from(pid) {
            Ok(pid) => process_start(pid).map_or(start_is_knowable(), |now| now != start),
            Err(_) => false,
        },
        _ => false,
    }
}

/// Whether this platform can read a process's start time at all — if it cannot, an
/// unreadable one proves nothing.
fn start_is_knowable() -> bool {
    process_start(std::process::id()).is_some()
}

/// A process's start time in clock ticks since boot (field 22 of `/proc/<pid>/stat`),
/// or `None` if there is no such process or the platform does not expose it.
#[cfg(target_os = "linux")]
fn process_start(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name is parenthesised and may itself contain spaces or `)`, so the
    // fixed-position fields are counted from after its last `)`: state is field 3,
    // start time field 22.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

#[cfg(not(target_os = "linux"))]
fn process_start(_pid: u32) -> Option<u64> {
    None
}

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    const LOCK: &str = "test.lock";

    fn scratch(tag: &str) -> PathBuf {
        let uniq = Box::into_raw(Box::new(0u8)) as usize;
        let dir =
            std::env::temp_dir().join(format!("dir-lock-{tag}-{}-{uniq}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_live_holder_keeps_the_lock_until_it_drops() {
        let dir = scratch("live");
        let held = DirLock::acquire(&dir, LOCK, "first").expect("free");
        let refused = DirLock::acquire(&dir, LOCK, "second").expect_err("held");
        assert!(refused.contains("in use by first"), "{refused}");
        drop(held);
        DirLock::acquire(&dir, LOCK, "second").expect("released on drop");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lock_left_by_a_dead_process_is_taken_over() {
        let dir = scratch("dead");
        // No process has this pid.
        std::fs::write(dir.join(LOCK), "http-gateway\npid=4294967294\nstart=1\n").unwrap();
        DirLock::acquire(&dir, LOCK, "restarted").expect("a dead holder's lock is stale");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reused_pid_with_another_start_time_is_stale() {
        let dir = scratch("reused");
        // Our own pid, but not our start time: a previous process that had this pid.
        let pid = std::process::id();
        let start = process_start(pid).expect("readable on linux");
        std::fs::write(
            dir.join(LOCK),
            format!("http-gateway\npid={pid}\nstart={}\n", start + 1),
        )
        .unwrap();
        DirLock::acquire(&dir, LOCK, "restarted").expect("the pid was reused");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unrecognised_lock_is_refused() {
        let dir = scratch("opaque");
        std::fs::write(dir.join(LOCK), "something else\n").unwrap();
        assert!(DirLock::acquire(&dir, LOCK, "gateway").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
