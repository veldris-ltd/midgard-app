//! Exclusive use of a directory of append-only journals, released when dropped.
//! [RC-09] [RC-10]
//!
//! Journals, ledgers and stores are written by one process at a time; a second writer
//! would interleave frames and the next open would fail closed. The gateway and
//! `mg-admin` hold an enclosure's state directory this way, and each mesh service holds
//! its own.
//!
//! A process killed outright (SIGKILL, an OOM kill, a container stop) never runs its
//! drop and leaves the lock file behind. The lock records its holder's pid, the
//! operating system it ran on and, on Linux, macOS and Windows, that process's start
//! time, so a later acquire can tell a live holder from a dead one — including a new
//! process that happens to reuse the pid, as a restarted container's pid 1 does — and
//! takes over only a lock whose holder is gone. On Linux the start time comes from
//! `/proc`; on macOS from `proc_pidinfo`, which also refuses to describe another user's
//! live process, and such a holder counts as running; on Windows from `GetProcessTimes`,
//! where a process that cannot be opened for any reason but "no such process" likewise
//! counts as running.
//!
//! A state directory may be bind-mounted into a container while a native process can
//! also reach it, and a container's pid means nothing on the host. So a lock recorded on
//! another operating system is never taken over, whatever its pid; nor, on macOS, is one
//! that records no system (older builds wrote none, and ran only on Linux, where such a
//! lock is still read as Linux's). Where the start time cannot be read, a leftover lock is
//! refused and an operator removes it after confirming nothing is running.

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
                let recorded_os = recorded_os(&existing);
                if recorded_os != Some(std::env::consts::OS) {
                    return Err(format!(
                        "{} may be in use by another system ({} on {}, such as a container); \
                         if nothing is running, remove {}",
                        dir.display(),
                        existing.lines().next().unwrap_or("another process"),
                        recorded_os.unwrap_or("an unrecorded system"),
                        path.display()
                    ));
                }
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
        let os = std::env::consts::OS;
        writeln!(file, "{holder}\npid={pid}\nstart={start}\nos={os}")?;
        file.sync_all()?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

/// The operating system a lock file says its holder ran on. Older builds recorded none and
/// ran only on Linux, so on Linux a lock without one is Linux's; elsewhere it is foreign.
fn recorded_os(lock: &str) -> Option<&str> {
    let recorded = lock
        .lines()
        .find_map(|line| line.strip_prefix("os="))
        .map(str::trim);
    if cfg!(target_os = "linux") {
        recorded.or(Some("linux"))
    } else {
        recorded
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
            Ok(pid) => process_start(pid).map_or_else(
                || start_is_knowable() && !may_be_running(pid),
                |now| now != start,
            ),
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

/// On Linux an unreadable `/proc/<pid>/stat` means there is no such process.
#[cfg(target_os = "linux")]
fn may_be_running(_pid: u32) -> bool {
    false
}

/// A process's start time in microseconds since the epoch (`pbi_start_tvsec` and
/// `pbi_start_tvusec` of `proc_pidinfo(PROC_PIDTBSDINFO)`), or the errno that refused it.
#[cfg(target_os = "macos")]
fn bsd_start(pid: u32) -> Result<u64, i32> {
    use std::mem::{size_of, MaybeUninit};

    // No process can have a pid outside `pid_t`.
    let pid = libc::pid_t::try_from(pid).map_err(|_| libc::ESRCH)?;
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: `info` is a writable `proc_bsdinfo` of exactly `size` bytes; the kernel
    // writes at most that many and returns how many it wrote.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written <= 0 {
        return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0));
    }
    if written != size {
        // A short description sets no errno and proves nothing: the holder may be running.
        return Err(libc::EIO);
    }
    // SAFETY: the kernel filled the whole struct, and every bit pattern is valid for it.
    let info = unsafe { info.assume_init() };
    Ok(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

#[cfg(target_os = "macos")]
fn process_start(pid: u32) -> Option<u64> {
    bsd_start(pid).ok()
}

/// macOS will not describe another user's process (EPERM), so only "no such process"
/// proves the holder gone.
#[cfg(target_os = "macos")]
fn may_be_running(pid: u32) -> bool {
    bsd_start(pid).map_or_else(|errno| errno != libc::ESRCH, |_| true)
}

/// What Windows says about a pid.
#[cfg(windows)]
#[derive(Debug, PartialEq, Eq)]
enum WinProbe {
    /// Running, created at this many 100 ns intervals since 1601 (a `FILETIME`).
    Running(u64),
    /// No such process, or one that has exited and is kept only by an open handle.
    Gone,
    /// Windows would not say (access denied, or a call failed): it may be running.
    Unknown,
}

#[cfg(windows)]
mod win {
    //! The few kernel32 calls a lock needs. std links kernel32 already; declaring them
    //! here keeps the runtime free of a Windows bindings crate.
    use std::ffi::c_void;

    pub type Handle = *mut c_void;

    pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    pub const STILL_ACTIVE: u32 = 259;
    /// What `OpenProcess` reports for a pid no process has.
    pub const ERROR_INVALID_PARAMETER: i32 = 87;

    #[repr(C)]
    #[derive(Default)]
    pub struct FileTime {
        pub low: u32,
        pub high: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> Handle;
        pub fn GetProcessTimes(
            process: Handle,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        pub fn GetExitCodeProcess(process: Handle, exit_code: *mut u32) -> i32;
        pub fn CloseHandle(object: Handle) -> i32;
    }

    /// A process's creation time as a `FILETIME` count, from a handle to it.
    pub fn creation_time(process: Handle) -> Option<u64> {
        let (mut creation, mut exit) = (FileTime::default(), FileTime::default());
        let (mut kernel, mut user) = (FileTime::default(), FileTime::default());
        // SAFETY: `process` is an open process handle and the four out-pointers are
        // distinct, writable `FILETIME`s.
        let ok =
            unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) };
        (ok != 0).then(|| (u64::from(creation.high) << 32) | u64::from(creation.low))
    }
}

#[cfg(windows)]
fn win_probe(pid: u32) -> WinProbe {
    // SAFETY: OpenProcess takes plain values and returns null or a handle we own.
    let process = unsafe { win::OpenProcess(win::PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(win::ERROR_INVALID_PARAMETER) => WinProbe::Gone,
            _ => WinProbe::Unknown,
        };
    }
    let mut code = 0u32;
    // SAFETY: `process` is open with query rights and `code` is a writable u32.
    let probe = if unsafe { win::GetExitCodeProcess(process, &mut code) } == 0 {
        WinProbe::Unknown
    } else if code != win::STILL_ACTIVE {
        WinProbe::Gone
    } else {
        // A process that exited with code 259 reads as running here; its creation time
        // still matches, so its lock is kept, never wrongly taken.
        win::creation_time(process).map_or(WinProbe::Unknown, WinProbe::Running)
    };
    // SAFETY: `process` is a handle we opened and close exactly once.
    unsafe { win::CloseHandle(process) };
    probe
}

#[cfg(windows)]
fn process_start(pid: u32) -> Option<u64> {
    match win_probe(pid) {
        WinProbe::Running(start) => Some(start),
        WinProbe::Gone | WinProbe::Unknown => None,
    }
}

/// Only "no such process" or a recorded exit proves the holder gone on Windows.
#[cfg(windows)]
fn may_be_running(pid: u32) -> bool {
    win_probe(pid) != WinProbe::Gone
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn process_start(_pid: u32) -> Option<u64> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn may_be_running(_pid: u32) -> bool {
    true
}

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
mod tests {
    use super::*;

    use std::env::consts::OS;

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
        std::fs::write(
            dir.join(LOCK),
            format!("http-gateway\npid=4294967294\nstart=1\nos={OS}\n"),
        )
        .unwrap();
        DirLock::acquire(&dir, LOCK, "restarted").expect("a dead holder's lock is stale");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reused_pid_with_another_start_time_is_stale() {
        let dir = scratch("reused");
        // Our own pid, but not our start time: a previous process that had this pid.
        let pid = std::process::id();
        let start = process_start(pid).expect("our own start time is readable");
        std::fs::write(
            dir.join(LOCK),
            format!("http-gateway\npid={pid}\nstart={}\nos={OS}\n", start + 1),
        )
        .unwrap();
        DirLock::acquire(&dir, LOCK, "restarted").expect("the pid was reused");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_live_holder_whose_start_time_is_hidden_keeps_the_lock() {
        // SAFETY: getuid has no preconditions and cannot fail.
        if unsafe { libc::getuid() } == 0 {
            return; // root may read every process's start time
        }
        let dir = scratch("hidden");
        // launchd is pid 1 and root's: macOS will not tell another user its start time.
        assert_eq!(bsd_start(1), Err(libc::EPERM));
        std::fs::write(dir.join(LOCK), "http-gateway\npid=1\nstart=1\nos=macos\n").unwrap();
        let refused = DirLock::acquire(&dir, LOCK, "gateway").expect_err("launchd is running");
        assert!(refused.contains("in use by http-gateway"), "{refused}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_reaped_holder_is_gone_and_its_lock_is_taken_over() {
        let dir = scratch("reaped");
        // A real pid, inside pid_t, whose process has exited and been reaped.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert_eq!(bsd_start(pid), Err(libc::ESRCH));
        std::fs::write(
            dir.join(LOCK),
            format!("http-gateway\npid={pid}\nstart=1\nos=macos\n"),
        )
        .unwrap();
        DirLock::acquire(&dir, LOCK, "restarted").expect("the holder has exited");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lock_recorded_on_another_system_is_refused_even_if_its_pid_is_dead() {
        let dir = scratch("foreign");
        let other = if OS == "linux" { "macos" } else { "linux" };
        std::fs::write(
            dir.join(LOCK),
            format!("http-gateway\npid=4294967294\nstart=1\nos={other}\n"),
        )
        .unwrap();
        let refused = DirLock::acquire(&dir, LOCK, "gateway").expect_err("foreign lock");
        assert!(refused.contains("another system"), "{refused}");
        assert!(refused.contains(other), "{refused}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_lock_recorded_on_windows_is_refused() {
        let dir = scratch("from-windows");
        std::fs::write(
            dir.join(LOCK),
            "http-gateway\npid=4294967294\nstart=1\nos=windows\n",
        )
        .unwrap();
        let refused = DirLock::acquire(&dir, LOCK, "gateway").expect_err("foreign lock");
        assert!(refused.contains("another system"), "{refused}");
        assert!(refused.contains("windows"), "{refused}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_lock_recording_no_system_is_foreign_on_macos() {
        let dir = scratch("unrecorded");
        // The format older builds wrote, which ran only on Linux.
        std::fs::write(dir.join(LOCK), "http-gateway\npid=4294967294\nstart=1\n").unwrap();
        let refused = DirLock::acquire(&dir, LOCK, "gateway").expect_err("no system recorded");
        assert!(refused.contains("another system"), "{refused}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_lock_from_an_older_linux_build_behaves_as_before() {
        let dir = scratch("legacy");
        // No `os=` line: a dead holder's lock is still taken over, a live one still kept.
        std::fs::write(dir.join(LOCK), "http-gateway\npid=4294967294\nstart=1\n").unwrap();
        DirLock::acquire(&dir, LOCK, "restarted").expect("a dead holder's lock is stale");
        let pid = std::process::id();
        let start = process_start(pid).expect("readable on linux");
        std::fs::write(
            dir.join(LOCK),
            format!("http-gateway\npid={pid}\nstart={start}\n"),
        )
        .unwrap();
        let refused = DirLock::acquire(&dir, LOCK, "second").expect_err("live holder");
        assert!(refused.contains("in use by http-gateway"), "{refused}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_lock_on_windows_records_the_system_and_a_start_time() {
        let dir = scratch("win-record");
        let held = DirLock::acquire(&dir, LOCK, "gateway").expect("free");
        let lock = std::fs::read_to_string(dir.join(LOCK)).unwrap();
        assert!(lock.contains("\nos=windows\n"), "{lock}");
        let start = process_start(std::process::id()).expect("our own start time");
        assert!(lock.contains(&format!("\nstart={start}\n")), "{lock}");
        drop(held);
        assert!(!dir.join(LOCK).exists(), "released on drop");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A child that stays alive until killed.
    #[cfg(windows)]
    fn long_lived_child() -> std::process::Child {
        std::process::Command::new("ping")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn a_live_holder_on_windows_keeps_the_lock_until_its_start_time_differs() {
        use std::os::windows::io::AsRawHandle;

        let dir = scratch("win-live");
        let mut child = long_lived_child();
        let pid = child.id();
        let start = win::creation_time(child.as_raw_handle()).expect("the child's start");
        assert_eq!(win_probe(pid), WinProbe::Running(start));
        std::fs::write(
            dir.join(LOCK),
            format!("http-gateway\npid={pid}\nstart={start}\nos=windows\n"),
        )
        .unwrap();
        let refused = DirLock::acquire(&dir, LOCK, "gateway").expect_err("the child runs");
        assert!(refused.contains("in use by http-gateway"), "{refused}");
        // The same pid with another start time: a previous process that had this pid.
        std::fs::write(
            dir.join(LOCK),
            format!("http-gateway\npid={pid}\nstart={}\nos=windows\n", start + 1),
        )
        .unwrap();
        let taken = DirLock::acquire(&dir, LOCK, "restarted");
        let _ = child.kill();
        let _ = child.wait();
        taken.expect("the pid was reused");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_lock_left_by_an_exited_windows_process_is_taken_over() {
        use std::os::windows::io::AsRawHandle;

        let dir = scratch("win-exited");
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        // Our handle keeps the exited process's record (and so its pid) alive: its real
        // start time is still readable, and Windows reports it exited.
        let start = win::creation_time(child.as_raw_handle()).expect("the child's start");
        assert_eq!(win_probe(pid), WinProbe::Gone);
        std::fs::write(
            dir.join(LOCK),
            format!("http-gateway\npid={pid}\nstart={start}\nos=windows\n"),
        )
        .unwrap();
        DirLock::acquire(&dir, LOCK, "restarted").expect("the holder has exited");
        drop(child);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_pid_no_windows_process_has_is_gone() {
        assert_eq!(win_probe(4_294_967_294), WinProbe::Gone);
    }

    #[cfg(windows)]
    #[test]
    fn a_lock_recording_no_system_is_foreign_on_windows() {
        let dir = scratch("win-unrecorded");
        std::fs::write(dir.join(LOCK), "http-gateway\npid=4294967294\nstart=1\n").unwrap();
        let refused = DirLock::acquire(&dir, LOCK, "gateway").expect_err("no system recorded");
        assert!(refused.contains("another system"), "{refused}");
        // And a lock from another system is refused here, whatever its pid.
        std::fs::write(
            dir.join(LOCK),
            "http-gateway\npid=4294967294\nstart=1\nos=macos\n",
        )
        .unwrap();
        let refused = DirLock::acquire(&dir, LOCK, "gateway").expect_err("macOS lock");
        assert!(refused.contains("macos"), "{refused}");
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
