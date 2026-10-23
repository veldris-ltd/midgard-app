use service_runtime::json::parse;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_midgard-replay");

/// A fresh temp dir, removed again when dropped.
struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// A recording of one `feed` batch at offset 0 and one Forge file at offset 1, an
/// identity dir holding `cert_for.cert`, a forge dir and a fake `heimdall-source`.
fn setup(name: &str, feed: &str, cert_for: &str) -> TempDir {
    let d = std::env::temp_dir().join(format!("replay-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    for sub in ["rec/forge", "ids", "forge"] {
        fs::create_dir_all(d.join(sub)).unwrap();
    }
    fs::write(
        d.join("rec/meta.json"),
        r#"{"version":1,"recordedFrom":1792000000,"duration":2}"#,
    )
    .unwrap();
    fs::write(
        d.join("rec/heimdall.jsonl"),
        format!(
            r#"{{"offset":0,"feed":"{feed}","observations":[{{"entity":"A1","kind":"air-track","observedAt":1792000000,"attrs":{{}}}}]}}"#
        ) + "\n",
    )
    .unwrap();
    fs::write(
        d.join("rec/forge/manifest.json"),
        r#"[{"offset":1,"file":"aircraft.csv","path":"forge/aircraft-1.csv"}]"#,
    )
    .unwrap();
    fs::write(
        d.join("rec/forge/aircraft-1.csv"),
        "key,time_raised\nA,1792000001\n",
    )
    .unwrap();
    fs::write(d.join(format!("ids/{cert_for}.cert")), "").unwrap();
    fs::write(d.join("ids/mesh.anchor"), "").unwrap();
    fake_source(&d);
    TempDir(d)
}

#[cfg(unix)]
const FAKE: &str = "heimdall-source";
#[cfg(unix)]
fn fake_source(d: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let fake = d.join(FAKE);
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$@\" > {0}/args\ncat > {0}/stdin\necho 'batch: 1 accepted, 0 refused, 0 unreadable'\n",
            d.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
}

/// On Windows the fake is a batch file that hands the same script to the `sh` of Git
/// for Windows, which the Windows CI job runs under; `sh` reads `\` as an escape.
#[cfg(windows)]
const FAKE: &str = "heimdall-source.cmd";
#[cfg(windows)]
fn fake_source(d: &Path) {
    fs::write(
        d.join("heimdall-source.sh"),
        format!(
            "echo \"$@\" > {0}/args\ncat > {0}/stdin\necho 'batch: 1 accepted, 0 refused, 0 unreadable'\n",
            d.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    fs::write(d.join(FAKE), "@sh \"%~dp0heimdall-source.sh\" %*\r\n").unwrap();
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

fn run(d: &Path, extra: &[&str]) -> Output {
    let p = |s: &str| d.join(s).display().to_string();
    let mut args = vec![
        "--recording".to_string(),
        p("rec"),
        "--identity-dir".into(),
        p("ids"),
        "--forge-dir".into(),
        p("forge"),
        "--source-bin".into(),
        p(FAKE),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    Command::new(BIN).args(&args).output().unwrap()
}

#[test]
fn plays_one_loop_through_heimdall_source_and_the_forge_dir() {
    if !have_sh("plays_one_loop_through_heimdall_source_and_the_forge_dir") {
        return;
    }
    let t = setup("loop", "testfeed", "testfeed");
    let start = now();
    let out = run(&t.0, &["--loops", "1"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("\"replay started\""), "{stdout}");
    let sent = stdout
        .lines()
        .find(|l| l.contains("\"batch sent\""))
        .unwrap_or_else(|| panic!("no batch sent: {stdout}"));
    assert!(sent.contains("1 accepted"), "{sent}");

    let args = fs::read_to_string(t.0.join("args")).unwrap();
    assert!(
        args.contains("--addr 127.0.0.1:7730 --service testfeed --batch -"),
        "{args}"
    );
    let stdin = fs::read_to_string(t.0.join("stdin")).unwrap();
    let at = parse(stdin.trim())
        .unwrap()
        .get("observedAt")
        .and_then(|v| v.as_i64())
        .unwrap();
    assert!(at >= start - 1 && at <= now(), "{at} vs {start}");

    let csv = fs::read_to_string(t.0.join("forge/aircraft.csv")).unwrap();
    let raised: i64 = csv.lines().nth(1).unwrap()[2..].parse().unwrap();
    assert!(raised >= start && raised <= now() + 1, "{csv} vs {start}");
}

#[test]
fn a_feed_without_an_identity_is_refused_before_playing() {
    let t = setup("noid", "nofeed", "otherfeed");
    let out = run(&t.0, &["--loops", "1"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("missing identity for feed nofeed"),
        "{stderr}"
    );
    assert!(!t.0.join("stdin").exists());
}

#[test]
fn an_identity_dir_without_the_mesh_anchor_is_refused() {
    let t = setup("noanchor", "testfeed", "testfeed");
    fs::remove_file(t.0.join("ids/mesh.anchor")).unwrap();
    let out = run(&t.0, &["--loops", "1"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("mesh.anchor"), "{stderr}");
    assert!(!t.0.join("stdin").exists());
}

#[test]
fn a_forge_dir_that_is_not_a_directory_is_refused() {
    let t = setup("noforge", "testfeed", "testfeed");
    fs::remove_dir(t.0.join("forge")).unwrap();
    let out = run(&t.0, &["--loops", "1"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not a directory"), "{stderr}");
}

#[test]
fn missing_or_bad_options_print_usage() {
    let out = Command::new(BIN)
        .args(["--identity-dir", "/x", "--forge-dir", "/y"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--recording is required"), "{stderr}");
    assert!(stderr.contains("usage: midgard-replay"), "{stderr}");

    let t = setup("badloops", "testfeed", "testfeed");
    let out = run(&t.0, &["--loops", "many"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: midgard-replay"));

    let out = run(&t.0, &["--loops", "0"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--loops"));

    let out = run(&t.0, &["--bogus", "1"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: midgard-replay"));
}
