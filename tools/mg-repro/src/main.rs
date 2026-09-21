//! mg-repro — reproducibility checker. Builds **twice**, under differing target
//! paths, a fixed build clock, a normalised environment and remapped source and
//! target paths, then asserts every emitted artefact is **bit-identical**. Fails
//! closed (non-zero) if any digest differs, blocking a non-reproducible release.
//! [AC-M-6] [NFR-OSS-1]
//!
//! ```text
//! mg-repro                     # the whole workspace (what CI runs)
//! mg-repro workspace           # the same, spelled out
//! mg-repro <package>           # one package
//! mg-repro --only a,b,c        # a named subset
//! mg-repro [--jobs N] [--out repro.json]
//! ```
//!
//! ## Why two whole-workspace builds, not two per package
//! Comparing N packages by building each twice is 2N builds; building the
//! workspace twice and comparing every artefact is 2, and covers strictly more —
//! library `.rlib`s included, not just the binaries. A reproducibility regression
//! in a library is caught before it reaches a binary.
//!
//! ## Normalisation
//! Anything that can leak the build environment into the output is pinned or
//! cleared. Each item below was chosen because it is *known* to leak, not
//! defensively:
//!
//! | Control | Why |
//! |---|---|
//! | `--remap-path-prefix` source → `/src` | absolute source paths appear in debug info and panic messages |
//! | `--remap-path-prefix` target → `/target` | ditto for generated and dependency paths |
//! | `SOURCE_DATE_EPOCH=1` | honoured by build scripts that stamp a date |
//! | `-Clink-arg=/Brepro` (MSVC) | the MSVC linker stamps a wall-clock timestamp into the PE header and the debug directory, and derives the PDB signature GUID from it. Measured: 20 differing bytes without it, zero with |
//! | `TZ=UTC`, `LC_ALL=C`, `LANG=C` | locale- and zone-dependent formatting in build scripts |
//! | `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER` cleared | sccache and friends can alter codegen inputs |
//! | `CARGO_BUILD_JOBS` pinned | parallelism can change codegen-unit partitioning |
//! | `CARGO_INCREMENTAL=0` | incremental artefacts are not reproducible by design |
//! | `HOSTNAME` pinned | build scripts that stamp the build host would otherwise differ per machine |
//!
//! **`PATH` ordering is deliberately not normalised.** Pinning it would mean
//! choosing a toolchain layout, which differs legitimately between the Nix CI
//! shell and a developer's machine — and a `PATH` that resolves different
//! binaries is a different build, not a formatting difference. The hermetic Nix
//! environment is what makes `PATH` reproducible for the run that matters; this
//! tool detects the consequences rather than pretending to control them.
//!
//! ## The exception list
//! `supply-chain/repro-exceptions.txt` names packages known to be
//! non-reproducible, each with an owner and a reason. A listed artefact is
//! reported and does **not** fail the run; anything else that differs does. The
//! list must be empty at GA — it exists so a newly-discovered regression can be
//! quarantined with a tracking item rather than silently weakening the check.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use pqc_agility::sha256;

/// Where the exception list lives, relative to the workspace root.
const EXCEPTIONS: &str = "supply-chain/repro-exceptions.txt";

/// One artefact's result across both builds.
struct Entry {
    /// File name as emitted, e.g. `mg-sbom.exe` or `libattest.rlib`.
    artefact: String,
    /// The workspace package it belongs to.
    package: String,
    /// Digest from the first build; empty if the build did not emit it.
    digest_run1: String,
    /// Digest from the second build; empty if the build did not emit it.
    digest_run2: String,
    /// Both builds emitted it and the digests match.
    reproducible: bool,
    /// Listed in the exception file, so a difference does not fail the run.
    excepted: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("mg-repro: {msg}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    /// Packages to build; empty means the whole workspace.
    only: Vec<String>,
    jobs: Option<String>,
    out: Option<PathBuf>,
    /// Where to copy the verified artefacts, so the thing that gets signed is
    /// the thing that was proven reproducible.
    emit: Option<PathBuf>,
}

fn parse_args() -> Result<Options, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut only: Vec<String> = Vec::new();
    let mut jobs = None;
    let mut out = None;
    let mut emit = None;

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                println!(
                    "usage: mg-repro [<package> | workspace] [--only a,b] [--jobs N] [--out repro.json]"
                );
                std::process::exit(0);
            }
            "--only" => {
                let v = argv.get(i + 1).ok_or("--only requires a value")?;
                only.extend(
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                );
                i += 2;
            }
            "--jobs" => {
                jobs = Some(argv.get(i + 1).ok_or("--jobs requires a value")?.clone());
                i += 2;
            }
            "--out" => {
                out = Some(PathBuf::from(
                    argv.get(i + 1).ok_or("--out requires a value")?,
                ));
                i += 2;
            }
            "--emit" => {
                emit = Some(PathBuf::from(
                    argv.get(i + 1).ok_or("--emit requires a directory")?,
                ));
                i += 2;
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown flag {other}"));
            }
            // A bare argument names one package. `workspace` means "all of it",
            // and is accepted rather than short-circuited so an existing
            // `just repro workspace` invocation does real work.
            "workspace" => i += 1,
            other => {
                only.push(other.to_string());
                i += 1;
            }
        }
    }
    Ok(Options {
        only,
        jobs,
        out,
        emit,
    })
}

fn run() -> Result<ExitCode, String> {
    let opts = parse_args()?;
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read cwd: {e}"))?;

    let scope = if opts.only.is_empty() {
        "workspace".to_string()
    } else {
        opts.only.join(",")
    };
    eprintln!("mg-repro: checking {scope} — building twice");

    let mut runs: Vec<BTreeMap<String, String>> = Vec::new();
    for run in 1..=2u32 {
        let target = std::env::temp_dir().join(format!("mg-repro-{run}"));
        build(&cwd, &target, &opts, run)?;
        runs.push(digest_artefacts(&target.join("release"))?);
    }

    let (first, second) = (&runs[0], &runs[1]);
    let exceptions = load_exceptions(&cwd);

    // A differing *set* of artefacts is itself a reproducibility failure: the
    // build emitted something one time and not the other.
    let mut names: Vec<&String> = first.keys().chain(second.keys()).collect();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return Err("no artefacts were produced — nothing was compared".into());
    }

    let mut report: Vec<Entry> = Vec::new();
    let (mut ok_count, mut bad, mut excepted) = (0usize, Vec::new(), Vec::new());

    for name in names {
        let a = first.get(name);
        let b = second.get(name);
        let matched = matches!((a, b), (Some(x), Some(y)) if x == y);
        let package = package_of(name);
        let is_excepted = exceptions.iter().any(|e| package == *e);
        report.push(Entry {
            artefact: name.clone(),
            package,
            digest_run1: a.cloned().unwrap_or_default(),
            digest_run2: b.cloned().unwrap_or_default(),
            reproducible: matched,
            excepted: is_excepted,
        });
        if matched {
            ok_count += 1;
        } else if is_excepted {
            excepted.push(name.clone());
        } else {
            bad.push(name.clone());
        }
    }

    // Every package in scope must have produced something. A member that emits
    // no artefact is not "reproducible by default" — it is a hole in the
    // coverage this check claims, and silence is exactly how such a hole
    // survives. [AC-RF-SC-3b]
    let uncovered = uncovered_members(&cwd, &opts, &report)?;

    for e in &report {
        let mark = match (e.reproducible, e.excepted) {
            (true, _) => "ok      ",
            (false, true) => "EXCEPTED",
            (false, false) => "DIFFERS ",
        };
        println!(
            "{mark}  {}  {}",
            e.artefact,
            &e.digest_run1[..e.digest_run1.len().min(16)]
        );
    }

    if let Some(path) = &opts.out {
        write_report(path, &report)?;
        eprintln!("mg-repro: wrote {}", path.display());
    }

    println!(
        "mg-repro: {ok_count} reproducible, {} differing, {} excepted, {} total across {} package(s)",
        bad.len(),
        excepted.len(),
        report.len(),
        report
            .iter()
            .map(|e| e.package.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );

    for name in &excepted {
        eprintln!("mg-repro: WARNING — {name} is a recorded exception (see {EXCEPTIONS})");
    }

    if !uncovered.is_empty() {
        for pkg in &uncovered {
            eprintln!("mg-repro: FAIL — {pkg} produced no comparable artefact (coverage hole)");
        }
        return Ok(ExitCode::FAILURE);
    }

    if bad.is_empty() {
        // Hand out the artefacts that were actually proven, not a separately
        // built copy. Signing anything else would produce a signature over a
        // binary nobody can reproduce, which defeats the point of proving it.
        if let Some(dir) = &opts.emit {
            let from = std::env::temp_dir().join("mg-repro-1").join("release");
            emit_verified(&from, dir, &report)?;
            eprintln!(
                "mg-repro: emitted {} verified artefact(s) to {}",
                report.len(),
                dir.display()
            );
        }
        println!("mg-repro: OK — every artefact is bit-reproducible");
        Ok(ExitCode::SUCCESS)
    } else {
        for name in &bad {
            eprintln!("mg-repro: FAIL — {name} is NOT reproducible (digests differ)");
        }
        Ok(ExitCode::FAILURE)
    }
}

/// Run one normalised build into `target`.
fn build(cwd: &Path, target: &Path, opts: &Options, run: u32) -> Result<(), String> {
    let mut rustflags = format!(
        "--remap-path-prefix={}=/src --remap-path-prefix={}=/target",
        cwd.display(),
        target.display()
    );
    if cfg!(target_env = "msvc") {
        rustflags.push_str(" -Clink-arg=/Brepro");
    }

    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--release", "--offline"]);
    if opts.only.is_empty() {
        cmd.arg("--workspace");
    } else {
        for p in &opts.only {
            cmd.args(["-p", p]);
        }
    }
    if let Some(j) = &opts.jobs {
        cmd.args(["-j", j]);
    }
    cmd.arg("--target-dir").arg(target);

    cmd.env("RUSTFLAGS", &rustflags)
        .env("SOURCE_DATE_EPOCH", "1")
        .env("CARGO_INCREMENTAL", "0")
        .env("TZ", "UTC")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("HOSTNAME", "midgard-build")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER");
    if let Some(j) = &opts.jobs {
        cmd.env("CARGO_BUILD_JOBS", j);
    }

    eprintln!("mg-repro: build {run} → {}", target.display());
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("build {run} failed ({s})")),
        Err(e) => Err(format!("cannot run cargo: {e}")),
    }
}

/// Digest every comparable artefact in a release directory.
///
/// Only the top level is walked: `deps/` holds per-unit outputs whose names
/// already carry a metadata hash, and `.d` (dependency lists) and `.pdb` (debug
/// symbols) are build side-channels, not shipped artefacts.
fn digest_artefacts(release: &Path) -> Result<BTreeMap<String, String>, String> {
    let entries = std::fs::read_dir(release)
        .map_err(|e| format!("cannot read {}: {e}", release.display()))?;

    let mut out = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read entry: {e}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_comparable(&path) {
            continue;
        }
        let bytes =
            std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        out.insert(name, hex(&sha256(&bytes)));
    }
    Ok(out)
}

/// Whether a release-directory file is a shipped artefact worth comparing.
fn is_comparable(path: &Path) -> bool {
    // Cargo's own bookkeeping (`.cargo-lock`, `.fingerprint`) sits alongside the
    // artefacts and has no extension, so it would otherwise be picked up as a
    // Unix executable.
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
    {
        return false;
    }
    match path.extension().and_then(|e| e.to_str()) {
        // Build side-channels, not artefacts.
        Some("d") | Some("pdb") | Some("exp") | Some("lib") => false,
        Some("exe") | Some("rlib") | Some("dll") | Some("so") | Some("dylib") | Some("a") => true,
        // No extension: a Unix executable.
        None => true,
        _ => false,
    }
}

/// Map an artefact file name back to a package label for the exception list.
fn package_of(artefact: &str) -> String {
    let stem = artefact
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(artefact);
    let stem = stem.strip_prefix("lib").unwrap_or(stem);
    stem.replace('_', "-")
}

/// Every package name declared by the workspace.
///
/// Parsed from the root `Cargo.toml`'s `members` list and each member's own
/// `Cargo.toml`, because a member's directory name is frequently not its package
/// name — `clients/sync` is `midgard-client-sync`, `crypto/attest` is `attest`.
/// Deriving the name from the path would silently mis-attribute those.
fn workspace_members(cwd: &Path) -> Result<Vec<String>, String> {
    let root = std::fs::read_to_string(cwd.join("Cargo.toml"))
        .map_err(|e| format!("cannot read workspace Cargo.toml: {e}"))?;

    // Take the `members = [ ... ]` array of the [workspace] table.
    let start = root
        .find("members")
        .and_then(|i| root[i..].find('[').map(|j| i + j + 1))
        .ok_or("workspace Cargo.toml has no members list")?;
    let end = start
        + root[start..]
            .find(']')
            .ok_or("workspace members list is unterminated")?;

    let mut names = Vec::new();
    for raw in root[start..end].split(',') {
        let dir = raw.trim().trim_matches('"').trim();
        if dir.is_empty() {
            continue;
        }
        let manifest = cwd.join(dir).join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest)
            .map_err(|e| format!("cannot read {}: {e}", manifest.display()))?;
        let name = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("name = \""))
            .map(|v| v.trim_end_matches('"').to_string())
            .ok_or_else(|| format!("{} declares no package name", manifest.display()))?;
        names.push(name);
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// Packages that were in scope but produced nothing to compare.
fn uncovered_members(cwd: &Path, opts: &Options, report: &[Entry]) -> Result<Vec<String>, String> {
    let produced: std::collections::BTreeSet<&str> =
        report.iter().map(|e| e.package.as_str()).collect();

    let expected = if opts.only.is_empty() {
        workspace_members(cwd)?
    } else {
        opts.only.clone()
    };

    Ok(expected
        .into_iter()
        .filter(|p| !produced.contains(p.as_str()))
        .collect())
}

/// Read the exception list: one package per line, `#` comments ignored.
fn load_exceptions(cwd: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(cwd.join(EXCEPTIONS)) else {
        return Vec::new();
    };
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Copy the verified artefacts out, and re-check each digest as it lands so a
/// truncated or racing copy cannot substitute something unproven.
fn emit_verified(from: &Path, to: &Path, report: &[Entry]) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    for e in report {
        let src = from.join(&e.artefact);
        let dst = to.join(&e.artefact);
        std::fs::copy(&src, &dst)
            .map_err(|err| format!("cannot copy {} → {}: {err}", src.display(), dst.display()))?;
        let bytes =
            std::fs::read(&dst).map_err(|err| format!("cannot read {}: {err}", dst.display()))?;
        let landed = hex(&sha256(&bytes));
        if landed != e.digest_run1 {
            return Err(format!(
                "{}: emitted copy digest {landed} does not match the verified {}",
                dst.display(),
                e.digest_run1
            ));
        }
    }
    Ok(())
}

/// Emit the machine-readable report `mg-sign` embeds into the DSSE predicate.
/// Sorted and timestamp-free, so the report is itself reproducible.
fn write_report(path: &Path, report: &[Entry]) -> Result<(), String> {
    let mut s = String::from("{\n  \"packages\": [\n");
    for (i, e) in report.iter().enumerate() {
        // Both digests are recorded, not just the first: when a run fails, the
        // report alone has to be enough to see *how* the two builds diverged,
        // without re-running anything.
        s.push_str(&format!(
            "    {{ \"package\": \"{}\", \"artefact\": \"{}\", \"sha256\": \"{}\", \
             \"digest_run1\": \"{}\", \"digest_run2\": \"{}\", \"reproducible\": {}, \"excepted\": {} }}",
            e.package, e.artefact, e.digest_run1, e.digest_run1, e.digest_run2, e.reproducible, e.excepted
        ));
        if i + 1 < report.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("  ]\n}\n");
    std::fs::write(path, s).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_label_strips_lib_prefix_and_extension() {
        assert_eq!(package_of("mg-sbom.exe"), "mg-sbom");
        assert_eq!(package_of("mg-sbom"), "mg-sbom");
        assert_eq!(package_of("libattest.rlib"), "attest");
        assert_eq!(package_of("libbifrost_artefact.rlib"), "bifrost-artefact");
    }

    #[test]
    fn only_shipped_artefacts_are_compared() {
        for yes in ["a.exe", "liba.rlib", "a.dll", "a.so", "a.dylib", "a"] {
            assert!(is_comparable(Path::new(yes)), "{yes} should be compared");
        }
        for no in [
            "a.d",
            "a.pdb",
            "a.exp",
            "a.lib",
            "a.json",
            "a.txt",
            // Cargo bookkeeping, not output.
            ".cargo-lock",
            ".fingerprint",
        ] {
            assert!(!is_comparable(Path::new(no)), "{no} should be skipped");
        }
    }

    #[test]
    fn exceptions_ignore_comments_and_blanks() {
        let dir = std::env::temp_dir().join("mg-repro-exceptions-test");
        std::fs::create_dir_all(dir.join("supply-chain")).unwrap();
        std::fs::write(
            dir.join(EXCEPTIONS),
            "# a comment\n\nflaky-crate  # owner: alice, tracked in RF-SC-3\n",
        )
        .unwrap();
        assert_eq!(load_exceptions(&dir), vec!["flaky-crate".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_exception_list_means_no_exceptions() {
        assert!(load_exceptions(Path::new("/nonexistent-dir-for-test")).is_empty());
    }

    fn entry(artefact: &str, d1: &str, d2: &str) -> Entry {
        Entry {
            artefact: artefact.into(),
            package: package_of(artefact),
            digest_run1: d1.into(),
            digest_run2: d2.into(),
            reproducible: d1 == d2,
            excepted: false,
        }
    }

    #[test]
    fn report_is_deterministic_and_parseable_by_mg_sign() {
        let dir = std::env::temp_dir().join("mg-repro-report-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("repro.json");
        let report = vec![
            entry("mg-sbom.exe", &"aa".repeat(32), &"aa".repeat(32)),
            entry("libattest.rlib", &"bb".repeat(32), &"bb".repeat(32)),
        ];
        write_report(&p, &report).unwrap();
        let first = std::fs::read_to_string(&p).unwrap();
        write_report(&p, &report).unwrap();
        assert_eq!(first, std::fs::read_to_string(&p).unwrap());

        // The shape mg-sign's --repro extractor looks for.
        assert!(first.contains("\"package\": \"mg-sbom\""));
        assert!(first.contains("\"sha256\": \""));
        assert!(!first.contains("timestamp"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn report_records_both_digests_so_a_failure_is_diagnosable() {
        // On a failure the report alone must show HOW the builds diverged; one
        // digest would only say THAT they did. [AC-RF-SC-3]
        let dir = std::env::temp_dir().join("mg-repro-both-digests-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("repro.json");
        let report = vec![entry("flaky.exe", &"aa".repeat(32), &"bb".repeat(32))];
        write_report(&p, &report).unwrap();

        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains(&format!("\"digest_run1\": \"{}\"", "aa".repeat(32))));
        assert!(text.contains(&format!("\"digest_run2\": \"{}\"", "bb".repeat(32))));
        assert!(text.contains("\"reproducible\": false"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_members_are_read_from_their_manifests_not_their_paths() {
        // A member's directory name is often not its package name, so deriving
        // the name from the path would mis-attribute artefacts. [AC-RF-SC-3b]
        let members = workspace_members(Path::new(".."))
            .or_else(|_| workspace_members(Path::new("../..")))
            .expect("read workspace members");

        assert!(members.contains(&"attest".to_string()), "crypto/attest");
        assert!(
            members.contains(&"midgard-client-sync".to_string()),
            "clients/sync is midgard-client-sync, not `sync`"
        );
        assert!(members.contains(&"model-server".to_string()));
        assert!(members.len() > 50, "got {} members", members.len());
    }

    #[test]
    fn a_package_that_emits_nothing_is_a_coverage_hole() {
        let opts = Options {
            only: vec!["ghost-package".into(), "mg-sbom".into()],
            jobs: None,
            out: None,
            emit: None,
        };
        let report = vec![entry("mg-sbom.exe", "aa", "aa")];
        let uncovered = uncovered_members(Path::new("."), &opts, &report).unwrap();
        assert_eq!(uncovered, vec!["ghost-package".to_string()]);
    }

    #[test]
    fn full_coverage_reports_no_holes() {
        let opts = Options {
            only: vec!["mg-sbom".into(), "attest".into()],
            jobs: None,
            out: None,
            emit: None,
        };
        let report = vec![
            entry("mg-sbom.exe", "aa", "aa"),
            entry("libattest.rlib", "bb", "bb"),
        ];
        assert!(uncovered_members(Path::new("."), &opts, &report)
            .unwrap()
            .is_empty());
    }
}
