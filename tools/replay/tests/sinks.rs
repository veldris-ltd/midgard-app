use midgard_replay::player::Sink;
use midgard_replay::sinks::LiveSink;
use std::path::{Path, PathBuf};
use std::{fs, os::unix::fs::PermissionsExt};

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
fn script(d: &Path, body: &str) -> PathBuf {
    let fake = d.join("heimdall-source");
    fs::write(&fake, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    fake
}

fn sink(d: &Path, source_bin: PathBuf) -> LiveSink {
    LiveSink {
        source_bin,
        identity_dir: "/ids".into(),
        addr: "127.0.0.1:7730".into(),
        forge_dir: d.join("forge"),
    }
}

#[test]
fn pipes_batches_to_heimdall_source_and_writes_forge_files_atomically() {
    let t = temp_dir("pipe");
    let d = &t.0;
    let fake = script(
        d,
        &format!(
            "echo \"$@\" > {0}/args\ncat > {0}/stdin\necho 'batch: 1 accepted, 0 refused, 0 unreadable'",
            d.display()
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
    let t = temp_dir("failed");
    let fake = script(&t.0, "echo 'error: connection refused' >&2\nexit 2");
    let err = sink(&t.0, fake).batch("f", "{}\n").unwrap_err();
    assert!(err.contains("error: connection refused"), "{err}");
}

#[test]
fn a_child_that_does_not_read_a_big_batch_still_reports() {
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
