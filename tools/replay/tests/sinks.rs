use midgard_replay::player::Sink;
use midgard_replay::sinks::{
    default_source_bin, rename_refusal_passes, rename_with_retry, LiveSink, RENAME_TRIES,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A fresh temp dir with a `forge` subdirectory, removed again when dropped.
struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn temp_dir(name: &str) -> TempDir {
    let d = std::env::temp_dir().join(format!("replay-sinks-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("forge")).unwrap();
    TempDir(d)
}

/// A fake `heimdall-source` in `d` running `body`.
#[cfg(unix)]
fn script(d: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let fake = d.join("heimdall-source");
    fs::write(&fake, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    fake
}

/// A fake `heimdall-source` in `d` running `body`: a batch file that hands it to the
/// `sh` of Git for Windows, which the Windows CI job runs under.
#[cfg(windows)]
fn script(d: &Path, body: &str) -> PathBuf {
    fs::write(d.join("heimdall-source.sh"), format!("{body}\n")).unwrap();
    let fake = d.join("heimdall-source.cmd");
    fs::write(&fake, "@sh \"%~dp0heimdall-source.sh\" %*\r\n").unwrap();
    fake
}

/// Whether this test's fake `heimdall-source` can run, else a message saying it is
/// skipped. On Windows the fakes are batch files that hand a script to the `sh` of Git
/// for Windows (the Windows CI job runs under Git Bash); where no `sh` is on PATH the
/// test is skipped with a clear message rather than failing on an unrelated error.
/// Always true on Unix, where the fakes are `/bin/sh` scripts.
fn have_sh(test: &str) -> bool {
    #[cfg(windows)]
    {
        let ok = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!(
                "skipping {test}: its fake heimdall-source needs the sh of Git for Windows \
                 on PATH (run the tests from Git Bash)"
            );
        }
        ok
    }
    #[cfg(not(windows))]
    {
        let _ = test;
        true
    }
}

/// `d` as the fake's shell writes it: `sh` reads a backslash as an escape.
fn sh_path(d: &Path) -> String {
    d.display().to_string().replace('\\', "/")
}

fn sink(d: &Path, source_bin: PathBuf) -> LiveSink {
    LiveSink {
        source_bin,
        identity_dir: "/ids".into(),
        addr: "127.0.0.1:7730".into(),
        forge_dir: d.join("forge"),
        timeout: Duration::from_secs(10),
    }
}

#[test]
fn pipes_batches_to_heimdall_source_and_writes_forge_files_atomically() {
    if !have_sh("pipes_batches_to_heimdall_source_and_writes_forge_files_atomically") {
        return;
    }
    let t = temp_dir("pipe");
    let d = &t.0;
    let fake = script(
        d,
        &format!(
            "echo \"$@\" > {0}/args\ncat > {0}/stdin\necho 'batch: 1 accepted, 0 refused, 0 unreadable'",
            sh_path(d)
        ),
    );
    let sink = sink(d, fake);

    assert_eq!(
        sink.batch("adsb-fi", "{\"a\":1}\n").unwrap(),
        "batch: 1 accepted, 0 refused, 0 unreadable"
    );
    assert_eq!(
        fs::read_to_string(d.join("args")).unwrap().trim(),
        "--identity-dir /ids --addr 127.0.0.1:7730 --service adsb-fi --batch -"
    );
    assert_eq!(fs::read_to_string(d.join("stdin")).unwrap(), "{\"a\":1}\n");

    sink.forge("aircraft.csv", "key\nA\n").unwrap();
    assert_eq!(
        fs::read_to_string(d.join("forge/aircraft.csv")).unwrap(),
        "key\nA\n"
    );
    // No temporary file is left beside it.
    assert_eq!(fs::read_dir(d.join("forge")).unwrap().count(), 1);
    assert!(sink.forge("../x.csv", "").is_err());
    assert!(sink.forge(".hidden.csv", "").is_err());
    assert!(sink.forge("", "").is_err());
}

#[test]
fn refusals_are_reported_not_failures() {
    if !have_sh("refusals_are_reported_not_failures") {
        return;
    }
    let t = temp_dir("refused");
    let fake = script(
        &t.0,
        "cat > /dev/null\necho 'batch: 1 accepted, 1 refused, 0 unreadable'\nexit 1",
    );
    assert_eq!(
        sink(&t.0, fake).batch("f", "{}\n{}\n").unwrap(),
        "batch: 1 accepted, 1 refused, 0 unreadable"
    );
}

#[test]
fn a_failed_submission_is_an_error_with_its_message() {
    if !have_sh("a_failed_submission_is_an_error_with_its_message") {
        return;
    }
    let t = temp_dir("failed");
    let fake = script(&t.0, "echo 'error: connection refused' >&2\nexit 2");
    let err = sink(&t.0, fake).batch("f", "{}\n").unwrap_err();
    assert!(err.contains("error: connection refused"), "{err}");
}

#[test]
fn a_child_that_does_not_read_a_big_batch_still_reports() {
    if !have_sh("a_child_that_does_not_read_a_big_batch_still_reports") {
        return;
    }
    let t = temp_dir("noread");
    let fake = script(&t.0, "echo 'error: bad identity' >&2\nexit 2");
    let big = "{\"a\":1}\n".repeat(200_000);
    let err = sink(&t.0, fake).batch("f", &big).unwrap_err();
    assert!(err.contains("error: bad identity"), "{err}");
}

#[test]
fn a_missing_binary_is_an_error_naming_it() {
    let t = temp_dir("missing");
    let bin = t.0.join("no-such-heimdall-source");
    let err = sink(&t.0, bin.clone()).batch("f", "{}\n").unwrap_err();
    assert!(err.contains(&bin.display().to_string()), "{err}");
}

#[test]
fn without_a_summary_the_error_is_stderr_s_last_line() {
    if !have_sh("without_a_summary_the_error_is_stderr_s_last_line") {
        return;
    }
    let t = temp_dir("stderr");
    let fake = script(
        &t.0,
        "echo 'connecting' \necho 'error: no route' >&2\necho '' >&2\nexit 2",
    );
    assert_eq!(
        sink(&t.0, fake).batch("f", "{}\n").unwrap_err(),
        "error: no route"
    );
}

#[test]
fn without_stderr_the_error_is_stdout_s_last_line() {
    if !have_sh("without_stderr_the_error_is_stdout_s_last_line") {
        return;
    }
    let t = temp_dir("stdout");
    let fake = script(&t.0, "echo 'first'\necho 'error: out'\nexit 2");
    assert_eq!(
        sink(&t.0, fake).batch("f", "{}\n").unwrap_err(),
        "error: out"
    );
}

#[test]
fn a_child_that_runs_too_long_is_killed() {
    if !have_sh("a_child_that_runs_too_long_is_killed") {
        return;
    }
    let t = temp_dir("slow");
    // A sleep no other process runs, to look for afterwards.
    let marker = format!("sleep 30.{}", std::process::id());
    let fake = script(&t.0, &format!("exec {marker}"));
    let s = LiveSink {
        timeout: Duration::from_secs(1),
        ..sink(&t.0, fake)
    };
    let start = Instant::now();
    let err = s.batch("f", "{}\n").unwrap_err();
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "{:?}",
        start.elapsed()
    );
    assert!(err.contains("timed out after 1 s"), "{err}");
    // Only the direct child is killed; on Windows that is the batch file's cmd.exe, and
    // there is no pgrep, so the leftover check is Unix only.
    #[cfg(unix)]
    {
        let found = std::process::Command::new("/usr/bin/pgrep")
            .args(["-f", &marker])
            .output()
            .unwrap();
        assert!(
            !found.status.success(),
            "still running: {}",
            String::from_utf8_lossy(&found.stdout)
        );
    }
}

#[test]
fn the_default_source_bin_is_beside_the_replay_with_the_platform_suffix() {
    let exe = Path::new("/opt/midgard/helpers")
        .join(format!("midgard-replay{}", std::env::consts::EXE_SUFFIX));
    let bin = default_source_bin(Some(&exe));
    assert_eq!(bin.parent(), Some(Path::new("/opt/midgard/helpers")));
    #[cfg(windows)]
    assert_eq!(bin.file_name().unwrap(), "heimdall-source.exe");
    #[cfg(not(windows))]
    assert_eq!(bin.file_name().unwrap(), "heimdall-source");
    // With no directory to look in, the bare name, for PATH.
    assert_eq!(
        default_source_bin(None),
        PathBuf::from(format!("heimdall-source{}", std::env::consts::EXE_SUFFIX))
    );
}

/// A rename that is refused `refusals` times with `err`, then succeeds.
fn refused(
    refusals: u32,
    err: i32,
) -> (
    impl FnMut() -> std::io::Result<()>,
    std::rc::Rc<std::cell::Cell<u32>>,
) {
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let c = calls.clone();
    let rename = move || {
        c.set(c.get() + 1);
        if c.get() <= refusals {
            Err(std::io::Error::from_raw_os_error(err))
        } else {
            Ok(())
        }
    };
    (rename, calls)
}

#[test]
fn a_refused_rename_that_passes_is_tried_again_with_backoff() {
    let (rename, calls) = refused(6, 32);
    let mut waits = Vec::new();
    rename_with_retry(rename, |_| true, |d| waits.push(d.as_millis())).unwrap();
    assert_eq!(calls.get(), 7);
    assert_eq!(waits, [50, 100, 200, 400, 500, 500]);
}

#[test]
fn a_rename_still_refused_after_every_try_returns_the_error() {
    let (rename, calls) = refused(u32::MAX, 32);
    let mut waits = 0;
    let err = rename_with_retry(rename, |_| true, |_| waits += 1).unwrap_err();
    assert_eq!(err.raw_os_error(), Some(32));
    assert_eq!(calls.get(), RENAME_TRIES);
    assert_eq!(waits, RENAME_TRIES - 1);
}

#[test]
fn a_rename_refused_for_good_is_not_tried_again() {
    let (rename, calls) = refused(u32::MAX, 2);
    let err = rename_with_retry(
        rename,
        |e| e.raw_os_error() == Some(32),
        |_| panic!("no wait expected"),
    )
    .unwrap_err();
    assert_eq!(err.raw_os_error(), Some(2));
    assert_eq!(calls.get(), 1);
}

#[test]
fn only_windows_sharing_and_access_refusals_pass() {
    let passes = |code| rename_refusal_passes(&std::io::Error::from_raw_os_error(code));
    // ERROR_SHARING_VIOLATION and ERROR_ACCESS_DENIED on Windows; on Unix 5 and 32
    // are EIO and EPIPE, and no rename failure is tried again.
    assert_eq!(passes(32), cfg!(windows));
    assert_eq!(passes(5), cfg!(windows));
    assert!(!passes(2));
}
