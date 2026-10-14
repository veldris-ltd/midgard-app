//! mg-repro — reproducibility checker. Builds **twice**, under differing target
//! paths, a fixed build clock, a normalised environment and remapped source and
//! target paths, then asserts every emitted artefact is **bit-identical**. Fails
//! closed (non-zero) if any digest differs, blocking a non-reproducible release.
//! [AC-M-6] [NFR-OSS-1]
//!
//! ```text
//! mg-repro                     # the whole workspace
//! mg-repro workspace           # the same, spelled out
//! mg-repro <package>           # one package
//! mg-repro --only a,b,c        # a named subset
//! mg-repro [--jobs N] [--out repro.json] [--emit <dir>]
//! ```
//!
//! ## Why two whole-workspace builds, not two per package
//! Comparing N packages by building each twice is 2N builds; building the
//! workspace twice and comparing every artefact is 2, and covers strictly more —
//! library `.rlib`s and `.rmeta`s included, not just the binaries. A
//! reproducibility regression in a library is caught before it reaches a binary.
//!
//! ## What is compared
//! Cargo is asked what it produced (`--message-format=json`), and exactly those
//! files are compared: every output of every lib, bin and proc-macro target of a
//! **workspace member**, attributed to the package whose manifest declares it.
//! Nothing is inferred from a directory listing or from a file name, so
//!
//! - a file left in the build directory by an earlier run — older source, a
//!   package since removed, a wider scope — is not an artefact of this run and is
//!   neither reported, nor counted as coverage, nor handed out by `--emit`;
//! - a binary whose name differs from its package (`bifrost-fleet` is built by
//!   `bifrost-fleet-server`, and is also the name of an unrelated library) is
//!   reported under the package that built it.
//!
//! The two build directories live under this workspace's own `target/mg-repro/`,
//! so they are private to the checkout rather than shared through the system
//! temp directory, and Cargo's own cache makes a re-run cheap.
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
//! | flags passed as `CARGO_ENCODED_RUSTFLAGS`; `RUSTFLAGS` and `CARGO_BUILD_RUSTFLAGS` cleared | the encoded form outranks every other source of flags, so an ambient value cannot silently replace the remapping — and a workspace path containing a space cannot split it |
//! | `SOURCE_DATE_EPOCH=1` | honoured by build scripts that stamp a date |
//! | `-Clink-arg=/Brepro` (MSVC) | the MSVC linker stamps a wall-clock timestamp into the PE header and the debug directory, and derives the PDB signature GUID from it. Measured: 20 differing bytes without it, zero with |
//! | `TZ=UTC`, `LC_ALL=C`, `LANG=C` | locale- and zone-dependent formatting in build scripts |
//! | `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER` cleared | sccache and friends can alter codegen inputs |
//! | `CARGO_BUILD_JOBS` and `-j` pinned, always (`--jobs`, default 4) | parallelism can change codegen-unit partitioning, so it must not follow the core count of whichever machine runs the build |
//! | `CARGO_INCREMENTAL=0` | incremental artefacts are not reproducible by design |
//! | `HOSTNAME` pinned | build scripts that stamp the build host would otherwise differ per machine |
//!
//! Pinning makes a value the *same* in both builds, which is the point — and it
//! means this check cannot see a leak of a pinned value. A binary that embeds
//! `HOSTNAME` builds identically twice here. What the two builds do differ in is
//! the wall clock, the target path, and anything random, and those are what a
//! failure here reports.
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
//! non-reproducible, each with an owner and a reason. A listed package's artefact
//! is reported and does **not** fail the run; anything else that differs does.
//! It is still never called reproducible: the verdict and `--emit` both say which
//! artefacts are exceptions. The list must be empty at GA — it exists so a
//! newly-discovered regression can be quarantined with a tracking item rather
//! than silently weakening the check.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use attest::json::{self, Json};
use pqc_agility::sha256;

/// Where the exception list lives, relative to the workspace root.
const EXCEPTIONS: &str = "supply-chain/repro-exceptions.txt";

/// Build parallelism when `--jobs` is not given. A fixed number, not the host's
/// core count, so the same source builds the same way on every machine.
const DEFAULT_JOBS: &str = "4";

const USAGE: &str =
    "usage: mg-repro [<package> | workspace] [--only a,b] [--jobs N] [--out repro.json] [--emit <dir>]";

/// One artefact's result across both builds.
struct Entry {
    /// The artefact's stable name, e.g. `mg-sbom.exe`, `libattest.rlib`.
    artefact: String,
    /// The workspace package whose manifest declares the target that built it.
    package: String,
    /// Digest from the first build; empty if the build did not emit it.
    digest_run1: String,
    /// Digest from the second build; empty if the build did not emit it.
    digest_run2: String,
    /// Both builds emitted it and the digests match.
    reproducible: bool,
    /// Its package is in the exception file, so a difference does not fail the run.
    excepted: bool,
    /// Where the first build left it, for `--emit`.
    path_run1: Option<PathBuf>,
}

/// One file a build produced.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Built {
    path: PathBuf,
    digest: String,
}

/// Everything one build produced, keyed by `(artefact, package)`.
type BuildOutputs = BTreeMap<(String, String), Built>;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // Help is its own invocation. It exits 0, so it must never be reachable from
    // a command line that also asks for a check: no argument value may produce a
    // success without a comparison. [AC-RF-SC-3a]
    if matches!(argv.as_slice(), [only] if is_help(only)) {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match parse_args(&argv).and_then(|opts| run(&opts)) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("mg-repro: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn is_help(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
}

#[derive(Debug)]
struct Options {
    /// Packages to build; empty means the whole workspace.
    only: Vec<String>,
    jobs: String,
    out: Option<PathBuf>,
    /// Where to copy the verified artefacts, so the thing that gets signed is
    /// the thing that was proven reproducible.
    emit: Option<PathBuf>,
}

fn parse_args(argv: &[String]) -> Result<Options, String> {
    let mut only: Vec<String> = Vec::new();
    let mut jobs = DEFAULT_JOBS.to_string();
    let mut out = None;
    let mut emit = None;

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            help if is_help(help) => {
                return Err(format!(
                    "{help} must be the only argument — it does not check anything"
                ));
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
                let v = argv.get(i + 1).ok_or("--jobs requires a value")?;
                if !v.parse::<u32>().is_ok_and(|n| n > 0) {
                    return Err(format!("--jobs must be a positive number, got {v:?}"));
                }
                jobs = v.clone();
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
            other if other.starts_with('-') => {
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

fn run(opts: &Options) -> Result<ExitCode, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read cwd: {e}"))?;

    let scope = if opts.only.is_empty() {
        "workspace".to_string()
    } else {
        opts.only.join(",")
    };
    eprintln!("mg-repro: checking {scope} — building twice");

    let members = workspace_members(&cwd)?;

    let mut runs: Vec<BuildOutputs> = Vec::new();
    for run in 1..=2u32 {
        let target = cwd.join("target").join("mg-repro").join(run.to_string());
        runs.push(build(&cwd, &target, opts, run, &members)?);
    }

    let (first, second) = (&runs[0], &runs[1]);
    let exceptions = load_exceptions(&cwd);

    // A differing *set* of artefacts is itself a reproducibility failure: the
    // build emitted something one time and not the other.
    let keys: BTreeSet<&(String, String)> = first.keys().chain(second.keys()).collect();
    if keys.is_empty() {
        return Err("no artefacts were produced — nothing was compared".into());
    }

    let mut report: Vec<Entry> = Vec::new();
    let (mut ok_count, mut bad, mut excepted) = (0usize, Vec::new(), Vec::new());

    for key in keys {
        let (artefact, package) = key;
        let a = first.get(key);
        let b = second.get(key);
        let matched = matches!((a, b), (Some(x), Some(y)) if x.digest == y.digest);
        let is_excepted = exceptions.iter().any(|e| e == package);
        report.push(Entry {
            artefact: artefact.clone(),
            package: package.clone(),
            digest_run1: a.map(|x| x.digest.clone()).unwrap_or_default(),
            digest_run2: b.map(|x| x.digest.clone()).unwrap_or_default(),
            reproducible: matched,
            excepted: is_excepted,
            path_run1: a.map(|x| x.path.clone()),
        });
        let name = format!("{artefact} ({package})");
        if matched {
            ok_count += 1;
        } else if is_excepted {
            excepted.push(name);
        } else {
            bad.push(name);
        }
    }

    // Every package in scope must have produced something. A member that emits
    // no artefact is not "reproducible by default" — it is a hole in the
    // coverage this check claims, and silence is exactly how such a hole
    // survives. [AC-RF-SC-3b]
    let uncovered = uncovered_members(&members, opts, &report);

    for e in &report {
        let mark = match (e.reproducible, e.excepted) {
            (true, _) => "ok      ",
            (false, true) => "EXCEPTED",
            (false, false) => "DIFFERS ",
        };
        println!(
            "{mark}  {}  {}  {}",
            e.package,
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
            .collect::<BTreeSet<_>>()
            .len()
    );

    for name in &excepted {
        eprintln!(
            "mg-repro: WARNING — {name} is NOT reproducible; it is a recorded exception (see {EXCEPTIONS})"
        );
    }
    for pkg in &uncovered {
        eprintln!("mg-repro: FAIL — {pkg} produced no comparable artefact (coverage hole)");
    }
    for name in &bad {
        eprintln!("mg-repro: FAIL — {name} is NOT reproducible (digests differ)");
    }
    if !uncovered.is_empty() || !bad.is_empty() {
        return Ok(ExitCode::FAILURE);
    }

    // Hand out the artefacts that were actually built and compared, not a
    // separately built copy. Signing anything else would produce a signature
    // over a binary nobody can reproduce, which defeats the point of proving it.
    if let Some(dir) = &opts.emit {
        emit_verified(dir, &report)?;
        eprintln!(
            "mg-repro: emitted {} artefact(s) to {} — {ok_count} proven reproducible, {} recorded exception(s)",
            report.len(),
            dir.display(),
            excepted.len()
        );
    }
    println!("mg-repro: {}", verdict(ok_count, excepted.len()));
    Ok(ExitCode::SUCCESS)
}

/// The passing verdict. It claims "every artefact" only when that is true: a run
/// that passes because of the exception list says so in the line a reader — or a
/// script grepping for it — sees last.
fn verdict(reproducible: usize, excepted: usize) -> String {
    if excepted == 0 {
        "OK — every artefact is bit-reproducible".to_string()
    } else {
        format!(
            "OK WITH EXCEPTIONS — {reproducible} artefact(s) are bit-reproducible; \
             {excepted} are NOT and pass only as recorded exceptions"
        )
    }
}

/// A workspace member, as Cargo reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Member {
    name: String,
    manifest: PathBuf,
}

/// Every package the workspace declares, from `cargo metadata`.
///
/// Cargo is the authority on what a member is — globs, excludes, renamed
/// packages and all — so it is asked rather than the root manifest being read by
/// hand. A member's directory name is frequently not its package name
/// (`clients/sync` is `midgard-client-sync`), which is also why artefacts are
/// attributed through the manifest path and never through a file name.
fn workspace_members(cwd: &Path) -> Result<Vec<Member>, String> {
    let out = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(cwd)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed ({})", out.status));
    }
    let text = String::from_utf8(out.stdout)
        .map_err(|_| "cargo metadata did not print UTF-8".to_string())?;
    parse_members(&text)
}

fn parse_members(metadata: &str) -> Result<Vec<Member>, String> {
    let doc = json::parse(metadata.trim()).map_err(|e| format!("cargo metadata: {e}"))?;
    let packages = doc
        .get("packages")
        .and_then(Json::as_array)
        .ok_or("cargo metadata lists no packages")?;

    let mut members = Vec::new();
    for p in packages {
        let field = |k: &str| {
            p.get(k)
                .and_then(Json::as_str)
                .ok_or_else(|| format!("cargo metadata: a package has no {k:?}"))
        };
        members.push(Member {
            name: field("name")?.to_string(),
            manifest: normalise(Path::new(field("manifest_path")?)),
        });
    }
    if members.is_empty() {
        return Err("the workspace has no members".into());
    }
    members.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(members)
}

/// A path in a form that compares equal however Cargo happened to spell it.
fn normalise(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Run one normalised build into `target` and return what it produced.
fn build(
    cwd: &Path,
    target: &Path,
    opts: &Options,
    run: u32,
    members: &[Member],
) -> Result<BuildOutputs, String> {
    let mut flags = vec![
        format!("--remap-path-prefix={}=/src", cwd.display()),
        format!("--remap-path-prefix={}=/target", target.display()),
    ];
    if cfg!(target_env = "msvc") {
        flags.push("-Clink-arg=/Brepro".to_string());
    }

    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--release", "--offline", "--message-format=json"]);
    if opts.only.is_empty() {
        cmd.arg("--workspace");
    } else {
        for p in &opts.only {
            cmd.args(["-p", p]);
        }
    }
    cmd.args(["-j", &opts.jobs]);
    cmd.arg("--target-dir").arg(target);

    // `CARGO_ENCODED_RUSTFLAGS` outranks `RUSTFLAGS`, the `build.rustflags`
    // config key and its environment form, so setting it is what guarantees
    // these flags are the ones in force. 0x1f separates flags, which also keeps
    // a path containing a space in one piece.
    cmd.env("CARGO_ENCODED_RUSTFLAGS", flags.join("\x1f"))
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_BUILD_RUSTFLAGS")
        .env("SOURCE_DATE_EPOCH", "1")
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_BUILD_JOBS", &opts.jobs)
        .env("TZ", "UTC")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("HOSTNAME", "midgard-build")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER");

    eprintln!("mg-repro: build {run} → {}", target.display());
    // stdout carries Cargo's JSON; its progress and any compiler output stay on
    // the terminal.
    let out = cmd
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !out.status.success() {
        return Err(format!("build {run} failed ({})", out.status));
    }
    let messages = String::from_utf8(out.stdout)
        .map_err(|_| format!("build {run}: cargo did not print UTF-8"))?;

    let produced = artefacts_in(&messages, members).map_err(|e| format!("build {run}: {e}"))?;
    let mut outputs = BuildOutputs::new();
    for ((artefact, package), path) in produced {
        let bytes =
            std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let digest = hex(&sha256(&bytes));
        outputs.insert((artefact, package), Built { path, digest });
    }
    Ok(outputs)
}

/// The comparable files a build produced, from Cargo's JSON messages.
///
/// One `compiler-artifact` message is emitted per target built *or found up to
/// date*, so the result is exactly this invocation's outputs whether or not
/// anything had to be recompiled. Only workspace members are kept — their
/// dependencies are inputs, pinned by `Cargo.lock`, not artefacts of this tree.
fn artefacts_in(
    messages: &str,
    members: &[Member],
) -> Result<BTreeMap<(String, String), PathBuf>, String> {
    let mut out = BTreeMap::new();
    for line in messages.lines().filter(|l| !l.trim().is_empty()) {
        let msg = json::parse(line.trim()).map_err(|e| format!("unreadable cargo message: {e}"))?;
        if msg.get("reason").and_then(Json::as_str) != Some("compiler-artifact") {
            continue;
        }
        let manifest = msg
            .get("manifest_path")
            .and_then(Json::as_str)
            .ok_or("a compiler-artifact message has no manifest_path")?;
        let manifest = normalise(Path::new(manifest));
        let Some(member) = members.iter().find(|m| m.manifest == manifest) else {
            continue;
        };
        let kinds: Vec<&str> = msg
            .get("target")
            .and_then(|t| t.get("kind"))
            .and_then(Json::as_array)
            .ok_or("a compiler-artifact message has no target kind")?
            .iter()
            .filter_map(Json::as_str)
            .collect();
        if !kinds.iter().any(|k| is_shipped_kind(k)) {
            continue;
        }
        let files = msg
            .get("filenames")
            .and_then(Json::as_array)
            .ok_or("a compiler-artifact message has no filenames")?;
        for file in files.iter().filter_map(Json::as_str) {
            let path = PathBuf::from(file);
            if !is_comparable(&path, &kinds) || !path.is_file() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.insert((stable_name(&name), member.name.clone()), path);
        }
    }
    Ok(out)
}

/// Whether a target kind produces something this workspace ships or links.
/// Build scripts, tests, benches and examples are tooling, not artefacts.
fn is_shipped_kind(kind: &str) -> bool {
    matches!(
        kind,
        "bin" | "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
    )
}

/// Whether one of a target's output files is an artefact worth comparing.
fn is_comparable(path: &Path, kinds: &[&str]) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        // Build side-channels, not artefacts.
        Some("d") | Some("pdb") | Some("exp") => false,
        // On MSVC `.lib` is a static library when the target is one, and an
        // import library — a side-channel of a DLL — otherwise.
        Some("lib") => kinds.contains(&"staticlib"),
        Some("exe") | Some("rlib") | Some("rmeta") | Some("dll") | Some("so") | Some("dylib")
        | Some("a") => true,
        // No extension: a Unix executable.
        None => true,
        _ => false,
    }
}

/// An artefact's name without the per-build metadata hash Cargo appends to
/// library outputs: `libattest-9dfe9bd5b80e74b4.rlib` → `libattest.rlib`.
///
/// The hash is derived from build inputs that are not guaranteed to match
/// between two target directories, so it is not part of an artefact's identity —
/// the two builds are paired by this name, and it is the name `--emit` writes.
fn stable_name(file_name: &str) -> String {
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) => (s, Some(e)),
        None => (file_name, None),
    };
    let stem = match stem.rsplit_once('-') {
        Some((head, hash)) if hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()) => {
            head
        }
        _ => stem,
    };
    match ext {
        Some(e) => format!("{stem}.{e}"),
        None => stem.to_string(),
    }
}

/// Packages that were in scope but produced nothing to compare.
fn uncovered_members(members: &[Member], opts: &Options, report: &[Entry]) -> Vec<String> {
    let produced: BTreeSet<&str> = report.iter().map(|e| e.package.as_str()).collect();

    let expected: Vec<String> = if opts.only.is_empty() {
        members.iter().map(|m| m.name.clone()).collect()
    } else {
        opts.only.clone()
    };

    expected
        .into_iter()
        .filter(|p| !produced.contains(p.as_str()))
        .collect()
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

/// Copy this run's artefacts out, and re-check each digest as it lands so a
/// truncated or racing copy cannot substitute something that was not compared.
fn emit_verified(to: &Path, report: &[Entry]) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    for e in report {
        let Some(src) = &e.path_run1 else {
            continue;
        };
        let dst = to.join(&e.artefact);
        std::fs::copy(src, &dst)
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

/// Minimal JSON string escaping (quotes, backslashes, control characters).
fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// Emit the machine-readable report `mg-sign` embeds into the provenance
/// attestation. Sorted and timestamp-free, so the report is itself reproducible.
fn write_report(path: &Path, report: &[Entry]) -> Result<(), String> {
    std::fs::write(path, render_report(report))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn render_report(report: &[Entry]) -> String {
    let mut s = String::from("{\n  \"packages\": [\n");
    for (i, e) in report.iter().enumerate() {
        // Both digests are recorded, not just the first: when a run fails, the
        // report alone has to be enough to see *how* the two builds diverged,
        // without re-running anything.
        s.push_str(&format!(
            "    {{ \"package\": \"{}\", \"artefact\": \"{}\", \"sha256\": \"{}\", \
             \"digest_run1\": \"{}\", \"digest_run2\": \"{}\", \"reproducible\": {}, \"excepted\": {} }}",
            json_escape(&e.package),
            json_escape(&e.artefact),
            e.digest_run1,
            e.digest_run1,
            e.digest_run2,
            e.reproducible,
            e.excepted
        ));
        if i + 1 < report.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("  ]\n}\n");
    s
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

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn opts(only: &[&str]) -> Options {
        Options {
            only: argv(only),
            jobs: DEFAULT_JOBS.into(),
            out: None,
            emit: None,
        }
    }

    fn member(name: &str, manifest: &str) -> Member {
        Member {
            name: name.into(),
            manifest: PathBuf::from(manifest),
        }
    }

    fn entry(package: &str, artefact: &str, d1: &str, d2: &str) -> Entry {
        Entry {
            artefact: artefact.into(),
            package: package.into(),
            digest_run1: d1.into(),
            digest_run2: d2.into(),
            reproducible: d1 == d2,
            excepted: false,
            path_run1: None,
        }
    }

    // --- arguments ---

    #[test]
    fn help_beside_anything_else_is_an_error_not_a_success() {
        // [AC-RF-SC-3a] no argument value may produce a success without a check.
        for bad in [
            vec!["workspace", "--help"],
            vec!["--help", "mg-sbom"],
            vec!["--only", "a,b", "-h"],
        ] {
            assert!(
                parse_args(&argv(&bad)).is_err(),
                "{bad:?} must not be treated as a request for help"
            );
        }
    }

    #[test]
    fn parallelism_is_always_pinned() {
        assert_eq!(parse_args(&argv(&[])).unwrap().jobs, DEFAULT_JOBS);
        assert_eq!(parse_args(&argv(&["--jobs", "8"])).unwrap().jobs, "8");
        for bad in ["0", "many", "-1", ""] {
            assert!(
                parse_args(&argv(&["--jobs", bad])).is_err(),
                "{bad:?} is not a job count"
            );
        }
    }

    #[test]
    fn scope_comes_from_the_bare_argument_or_only() {
        assert!(parse_args(&argv(&["workspace"])).unwrap().only.is_empty());
        assert_eq!(parse_args(&argv(&["mg-sbom"])).unwrap().only, ["mg-sbom"]);
        assert_eq!(
            parse_args(&argv(&["--only", "a, b,,c"])).unwrap().only,
            ["a", "b", "c"]
        );
        assert!(parse_args(&argv(&["--nonsense"])).is_err());
    }

    // --- what is compared, and who it belongs to ---

    /// A `compiler-artifact` line in the shape Cargo prints.
    fn artifact_line(manifest: &str, kind: &str, name: &str, files: &[&str]) -> String {
        let files: Vec<String> = files
            .iter()
            .map(|f| format!("\"{}\"", json_escape(f)))
            .collect();
        format!(
            "{{\"reason\":\"compiler-artifact\",\"manifest_path\":\"{}\",\
             \"target\":{{\"kind\":[\"{kind}\"],\"name\":\"{name}\"}},\
             \"filenames\":[{}],\"executable\":null,\"fresh\":false}}",
            json_escape(manifest),
            files.join(",")
        )
    }

    /// A scratch directory with real files in it, since only files that exist
    /// are artefacts.
    fn scratch(tag: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mg-repro-unit-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), f.as_bytes()).unwrap();
        }
        dir
    }

    #[test]
    fn artefacts_are_attributed_by_manifest_not_by_file_name() {
        // `bifrost-fleet` is a binary of the package `bifrost-fleet-server` AND
        // the name of an unrelated library package. Read from the file name, the
        // binary would be credited to the library.
        let dir = scratch(
            "attribution",
            &[
                "bifrost-fleet.exe",
                "libbifrost_fleet-0123456789abcdef.rlib",
                "libbifrost_fleet-0123456789abcdef.rmeta",
            ],
        );
        let p = |f: &str| dir.join(f).to_string_lossy().into_owned();
        let members = [
            member("bifrost-fleet", "/ws/bifrost/fleet/Cargo.toml"),
            member(
                "bifrost-fleet-server",
                "/ws/bifrost/fleet-server/Cargo.toml",
            ),
        ];
        let messages = [
            artifact_line(
                "/ws/bifrost/fleet-server/Cargo.toml",
                "bin",
                "bifrost-fleet",
                &[&p("bifrost-fleet.exe")],
            ),
            artifact_line(
                "/ws/bifrost/fleet/Cargo.toml",
                "lib",
                "bifrost_fleet",
                &[
                    &p("libbifrost_fleet-0123456789abcdef.rlib"),
                    &p("libbifrost_fleet-0123456789abcdef.rmeta"),
                ],
            ),
        ]
        .join("\n");

        let got = artefacts_in(&messages, &members).unwrap();
        let keys: Vec<(&str, &str)> = got.keys().map(|(a, p)| (a.as_str(), p.as_str())).collect();
        assert_eq!(
            keys,
            [
                ("bifrost-fleet.exe", "bifrost-fleet-server"),
                ("libbifrost_fleet.rlib", "bifrost-fleet"),
                ("libbifrost_fleet.rmeta", "bifrost-fleet"),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_this_builds_own_outputs_count() {
        // A stale file beside the real output, a dependency's artefact, a build
        // script, and the side-channels of a real target: none is an artefact of
        // this run.
        let dir = scratch(
            "scope",
            &[
                "mg-sbom.exe",
                "mg_sbom.pdb",
                "mg-sbom.d",
                "left-over-from-last-week.exe",
                "libserde-0123456789abcdef.rlib",
                "build-script-build.exe",
            ],
        );
        let p = |f: &str| dir.join(f).to_string_lossy().into_owned();
        let members = [member("mg-sbom", "/ws/tools/mg-sbom/Cargo.toml")];
        let messages = [
            artifact_line(
                "/ws/tools/mg-sbom/Cargo.toml",
                "bin",
                "mg-sbom",
                &[&p("mg-sbom.exe"), &p("mg_sbom.pdb"), &p("mg-sbom.d")],
            ),
            artifact_line(
                "/registry/serde/Cargo.toml",
                "lib",
                "serde",
                &[&p("libserde-0123456789abcdef.rlib")],
            ),
            artifact_line(
                "/ws/tools/mg-sbom/Cargo.toml",
                "custom-build",
                "build-script-build",
                &[&p("build-script-build.exe")],
            ),
            // Other message kinds are not artefacts.
            "{\"reason\":\"build-finished\",\"success\":true}".to_string(),
        ]
        .join("\n");

        let got = artefacts_in(&messages, &members).unwrap();
        let names: Vec<&str> = got.keys().map(|(a, _)| a.as_str()).collect();
        assert_eq!(names, ["mg-sbom.exe"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_cargo_message_fails_the_run() {
        // Guessing at a message we cannot parse would mean guessing at what was
        // built.
        assert!(artefacts_in("{\"reason\":\"compiler-artifact\"", &[]).is_err());
        assert!(artefacts_in("warning: not json", &[]).is_err());
        let no_manifest = "{\"reason\":\"compiler-artifact\",\"filenames\":[]}";
        assert!(artefacts_in(no_manifest, &[]).is_err());
    }

    #[test]
    fn the_metadata_hash_is_not_part_of_an_artefacts_name() {
        assert_eq!(
            stable_name("libattest-9dfe9bd5b80e74b4.rlib"),
            "libattest.rlib"
        );
        assert_eq!(
            stable_name("libattest-9dfe9bd5b80e74b4.rmeta"),
            "libattest.rmeta"
        );
        // Not a hash: wrong length, or not hex, or a real hyphenated name.
        assert_eq!(stable_name("mg-sbom.exe"), "mg-sbom.exe");
        assert_eq!(stable_name("mg-sbom"), "mg-sbom");
        assert_eq!(stable_name("bifrost-fleet-server"), "bifrost-fleet-server");
        assert_eq!(
            stable_name("tool-0123456789abcdeg.exe"),
            "tool-0123456789abcdeg.exe"
        );
    }

    #[test]
    fn side_channels_are_skipped_and_metadata_is_compared() {
        let lib = ["lib"];
        for yes in [
            "a.exe",
            "liba.rlib",
            "liba.rmeta",
            "a.dll",
            "a.so",
            "a.dylib",
            "a",
        ] {
            assert!(
                is_comparable(Path::new(yes), &lib),
                "{yes} should be compared"
            );
        }
        for no in ["a.d", "a.pdb", "a.exp", "a.json", "a.txt"] {
            assert!(
                !is_comparable(Path::new(no), &lib),
                "{no} should be skipped"
            );
        }
        // `.lib` is the artefact of a static library and a side-channel of a DLL.
        assert!(is_comparable(Path::new("a.lib"), &["staticlib"]));
        assert!(!is_comparable(Path::new("a.lib"), &["cdylib"]));
    }

    // --- members and coverage ---

    #[test]
    fn workspace_members_come_from_cargo_not_from_directory_names() {
        // A member's directory name is often not its package name. [AC-RF-SC-3b]
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let members = workspace_members(&root).expect("cargo metadata");
        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();

        assert!(names.contains(&"attest"), "crypto/attest");
        assert!(
            names.contains(&"midgard-client-sync"),
            "clients/sync is midgard-client-sync, not `sync`"
        );
        assert!(names.contains(&"bifrost-fleet") && names.contains(&"bifrost-fleet-server"));
        assert!(names.len() > 50, "got {} members", names.len());
        assert!(members.iter().all(|m| m.manifest.ends_with("Cargo.toml")));
    }

    #[test]
    fn member_parsing_refuses_metadata_it_cannot_read() {
        assert!(parse_members("{}").is_err());
        assert!(parse_members(r#"{"packages": []}"#).is_err());
        assert!(parse_members(r#"{"packages": [{"name": "a"}]}"#).is_err());
        assert!(parse_members(r#"{"packages": [{"name": "a", "manifest_path": "x"}"#).is_err());
        let ok = parse_members(
            r#"{"packages": [{"name": "b", "manifest_path": "/w/b/Cargo.toml"},
                             {"name": "a", "manifest_path": "/w/a/Cargo.toml"}]}"#,
        )
        .unwrap();
        assert_eq!(ok[0].name, "a", "sorted by name");
    }

    #[test]
    fn a_package_that_emits_nothing_is_a_coverage_hole() {
        let members = [member("mg-sbom", "m"), member("ghost-package", "g")];
        let report = vec![entry("mg-sbom", "mg-sbom.exe", "aa", "aa")];

        assert_eq!(
            uncovered_members(&members, &opts(&[]), &report),
            ["ghost-package"]
        );
        assert_eq!(
            uncovered_members(&members, &opts(&["ghost-package", "mg-sbom"]), &report),
            ["ghost-package"]
        );
    }

    #[test]
    fn another_packages_same_named_binary_is_not_coverage() {
        // The binary `bifrost-fleet` belongs to `bifrost-fleet-server`. It must
        // not stand in for the library package of the same name.
        let members = [
            member("bifrost-fleet", "a"),
            member("bifrost-fleet-server", "b"),
        ];
        let report = vec![entry(
            "bifrost-fleet-server",
            "bifrost-fleet.exe",
            "aa",
            "aa",
        )];
        assert_eq!(
            uncovered_members(&members, &opts(&[]), &report),
            ["bifrost-fleet"]
        );
    }

    #[test]
    fn full_coverage_reports_no_holes() {
        let members = [member("mg-sbom", "m"), member("attest", "a")];
        let report = vec![
            entry("mg-sbom", "mg-sbom.exe", "aa", "aa"),
            entry("attest", "libattest.rlib", "bb", "bb"),
        ];
        assert!(uncovered_members(&members, &opts(&[]), &report).is_empty());
    }

    // --- exceptions ---

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

    #[test]
    fn a_pass_by_exception_is_never_called_reproducible() {
        assert_eq!(verdict(57, 0), "OK — every artefact is bit-reproducible");
        let v = verdict(56, 1);
        assert!(!v.contains("every artefact"), "{v}");
        assert!(v.contains("1 are NOT"), "{v}");
    }

    // --- the report ---

    #[test]
    fn report_is_deterministic_and_parseable_by_mg_sign() {
        let report = vec![
            entry("mg-sbom", "mg-sbom.exe", &"aa".repeat(32), &"aa".repeat(32)),
            entry(
                "attest",
                "libattest.rlib",
                &"bb".repeat(32),
                &"bb".repeat(32),
            ),
        ];
        let text = render_report(&report);
        assert_eq!(text, render_report(&report));
        assert!(!text.contains("timestamp"));

        // mg-sign reads it with the same strict parser, and needs these fields.
        let doc = json::parse(text.trim()).expect("valid JSON");
        let rows = doc.get("packages").and_then(Json::as_array).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].get("package").and_then(Json::as_str),
            Some("mg-sbom")
        );
        assert_eq!(
            rows[0].get("reproducible").and_then(Json::as_bool),
            Some(true)
        );
        assert_eq!(rows[0].get("excepted").and_then(Json::as_bool), Some(false));
        assert_eq!(
            rows[0].get("sha256").and_then(Json::as_str),
            Some("aa".repeat(32).as_str())
        );
    }

    #[test]
    fn report_records_both_digests_so_a_failure_is_diagnosable() {
        // On a failure the report alone must show HOW the builds diverged; one
        // digest would only say THAT they did. [AC-RF-SC-3]
        let text = render_report(&[entry(
            "flaky",
            "flaky.exe",
            &"aa".repeat(32),
            &"bb".repeat(32),
        )]);
        assert!(text.contains(&format!("\"digest_run1\": \"{}\"", "aa".repeat(32))));
        assert!(text.contains(&format!("\"digest_run2\": \"{}\"", "bb".repeat(32))));
        assert!(text.contains("\"reproducible\": false"));
    }

    #[test]
    fn report_stays_valid_json_whatever_an_artefact_is_called() {
        let text = render_report(&[entry("odd\"pkg", "we\\ird \"name\".exe", "aa", "aa")]);
        let doc = json::parse(text.trim()).expect("still valid JSON");
        let row = &doc.get("packages").and_then(Json::as_array).unwrap()[0];
        assert_eq!(row.get("package").and_then(Json::as_str), Some("odd\"pkg"));
        assert_eq!(
            row.get("artefact").and_then(Json::as_str),
            Some("we\\ird \"name\".exe")
        );
    }
}
