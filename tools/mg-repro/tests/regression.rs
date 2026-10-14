//! RF-SC-3 — the reproducibility check is not vacuous. AC-RF-SC-3a … 3c.
//!
//! These run the **actual binary** against a throwaway workspace and assert its
//! exit code, because that is what a release gate consumes. The fixtures build
//! for real — two release builds each — so this is the regression test for the
//! claim the whole item rests on: a non-reproducible artefact turns the check
//! red and is named, and nothing but a real comparison turns it green.
//!
//! Everything here runs offline: the fixture crates have no dependencies.
//! `[NFR-PRV-1]`

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use attest::json::{self, Json};

const EXE: &str = env!("CARGO_BIN_EXE_mg-repro");

/// A throwaway Cargo workspace in the system temp directory.
struct Workspace(PathBuf);

impl Workspace {
    fn new(tag: &str, members: &[&str]) -> Self {
        let dir = std::env::temp_dir().join(format!("mg-repro-fixture-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture dir");

        let list: Vec<String> = members.iter().map(|m| format!("\"{m}\"")).collect();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[workspace]\nresolver = \"2\"\nmembers = [{}]\n",
                list.join(", ")
            ),
        )
        .unwrap();
        let ws = Self(dir);
        for m in members {
            match *m {
                "steady" => ws.add_steady(),
                "stamped" => ws.add_stamped(),
                other => panic!("no fixture crate called {other}"),
            }
        }
        ws
    }

    fn manifest(&self, name: &str) {
        let dir = self.0.join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
    }

    /// A crate that builds the same way every time.
    fn add_steady(&self) {
        self.manifest("steady");
        std::fs::write(
            self.0.join("steady/src/main.rs"),
            "fn main() {\n    println!(\"steady\");\n}\n",
        )
        .unwrap();
    }

    /// A crate whose build script stamps the wall clock, in nanoseconds, into
    /// the binary — the injected non-determinism of AC-RF-SC-3c. It reads the
    /// real clock, not `SOURCE_DATE_EPOCH`, so normalisation does not hide it.
    fn add_stamped(&self) {
        self.manifest("stamped");
        std::fs::write(
            self.0.join("stamped/build.rs"),
            r#"fn main() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let out = std::env::var("OUT_DIR").unwrap();
    std::fs::write(
        format!("{out}/stamp.rs"),
        format!("pub const STAMP: u128 = {nanos};"),
    )
    .unwrap();
}
"#,
        )
        .unwrap();
        std::fs::write(
            self.0.join("stamped/src/main.rs"),
            "include!(concat!(env!(\"OUT_DIR\"), \"/stamp.rs\"));\n\nfn main() {\n    println!(\"{STAMP}\");\n}\n",
        )
        .unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(EXE)
            .args(args)
            .current_dir(&self.0)
            .output()
            .expect("run mg-repro")
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    /// The report's rows as `(package, artefact, reproducible, run1, run2)`.
    fn report(&self) -> Vec<(String, String, bool, String, String)> {
        let text = std::fs::read_to_string(self.path("repro.json")).expect("repro.json written");
        let doc = json::parse(text.trim()).expect("repro.json is valid JSON");
        doc.get("packages")
            .and_then(Json::as_array)
            .expect("packages array")
            .iter()
            .map(|r| {
                let s = |k: &str| r.get(k).and_then(Json::as_str).unwrap().to_string();
                (
                    s("package"),
                    s("artefact"),
                    r.get("reproducible").and_then(Json::as_bool).unwrap(),
                    s("digest_run1"),
                    s("digest_run2"),
                )
            })
            .collect()
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// File names in a directory, or nothing if it does not exist.
fn listing(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// --- AC-RF-SC-3a / 3b — a pass means a real comparison of this run's outputs --

#[test]
fn a_reproducible_workspace_passes_and_only_its_own_outputs_count() {
    let ws = Workspace::new("clean", &["steady"]);

    let out = ws.run(&["--out", "repro.json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("OK — every artefact is bit-reproducible"),
        "{}",
        stdout(&out)
    );

    let rows = ws.report();
    assert_eq!(rows.len(), 1, "{rows:?}");
    let (package, artefact, reproducible, run1, run2) = &rows[0];
    assert_eq!(package, "steady");
    assert!(artefact.starts_with("steady"), "{artefact}");
    assert!(reproducible);
    assert_eq!(run1.len(), 64, "a real SHA-256 from a real build");
    assert_eq!(run1, run2);

    // A file left in the build directories by some earlier run is not an
    // artefact of this one. Plant an identical one in both: read from a
    // directory listing it would be "reproducible", reported, and emitted.
    for run in ["1", "2"] {
        let release = ws.path(&format!("target/mg-repro/{run}/release"));
        std::fs::write(release.join("left-over.exe"), b"from last week").unwrap();
        std::fs::write(release.join("libleft_over.rlib"), b"from last week").unwrap();
    }
    let out = ws.run(&["--out", "repro.json", "--emit", "verified"]);
    assert!(out.status.success(), "{}", stderr(&out));

    let rows = ws.report();
    assert_eq!(
        rows.len(),
        1,
        "the stale files must not be reported: {rows:?}"
    );
    assert_eq!(
        listing(&ws.path("verified")),
        [rows[0].1.clone()],
        "only this run's artefact is handed out"
    );

    // Naming a package that built nothing is a coverage hole, not a pass — and
    // the stale `left-over` file does not stand in for it.
    let out = ws.run(&["left-over"]);
    assert!(
        !out.status.success(),
        "a package that does not exist passed"
    );

    // Help beside a real request is an error, never a success without a check.
    let out = ws.run(&["workspace", "--help"]);
    assert!(!out.status.success(), "--help turned a check into a pass");
    assert!(stdout(&out).is_empty());
}

// --- AC-RF-SC-3c — an injected non-determinism turns the check red -----------

#[test]
fn an_injected_non_determinism_fails_the_run_and_names_the_package() {
    let ws = Workspace::new("stamped", &["steady", "stamped"]);

    let out = ws.run(&["--out", "repro.json", "--emit", "verified"]);
    let err = stderr(&out);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a non-reproducible artefact MUST fail the run: {err}"
    );
    assert!(
        err.contains("(stamped) is NOT reproducible"),
        "the failure must name the package: {err}"
    );
    assert!(
        !stdout(&out).contains("OK"),
        "nothing may claim success: {}",
        stdout(&out)
    );

    // The report is written even on failure, and shows how the builds diverged.
    let rows = ws.report();
    let row = |pkg: &str| rows.iter().find(|r| r.0 == pkg).expect("package in report");
    let (_, _, reproducible, run1, run2) = row("stamped");
    assert!(!reproducible);
    assert_ne!(run1, run2, "two different digests, both recorded");
    assert!(
        row("steady").2,
        "the unaffected package still reports clean"
    );

    // Nothing is handed to the signing stage from a failed run.
    assert!(
        listing(&ws.path("verified")).is_empty(),
        "a failed run emitted artefacts"
    );

    // Recorded as an exception, the same difference no longer fails the run —
    // and is still never described as reproducible.
    std::fs::create_dir_all(ws.path("supply-chain")).unwrap();
    std::fs::write(
        ws.path("supply-chain/repro-exceptions.txt"),
        "stamped   # owner: test, tracked in RF-SC-3\n",
    )
    .unwrap();
    let out = ws.run(&["--out", "repro.json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("OK WITH EXCEPTIONS"), "{text}");
    assert!(
        !text.contains("every artefact is bit-reproducible"),
        "a pass by exception must not claim every artefact: {text}"
    );
    assert!(stderr(&out).contains("(stamped) is NOT reproducible"));
    assert!(!ws.report().iter().find(|r| r.0 == "stamped").unwrap().2);
}
