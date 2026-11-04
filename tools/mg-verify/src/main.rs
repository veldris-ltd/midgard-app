//! mg-verify — verify an artefact's ML-DSA signature, SBOM, and provenance
//! before publish or deploy. FAILS CLOSED. [NFR-PQ-3] [AC-B-1] [AC-B-4]
//!
//! This binary is argument handling and an exit code over
//! [`attest::verify_artefact`], the file-level verifier. It runs in CI (stage 7,
//! pre-publish) and, as `--run-service`, as every release image's entrypoint:
//! it verifies the service binary and only then becomes it.
//! Bifrost's pre-deploy check (AC-B-1) and Mimir's model load (AC-MS-2) verify
//! their own manifests rather than files, so they do not call this — but all of
//! them reach the same signature decision, `attest::accept::accept`: one
//! implementation of "reject unless proven".
//!
//! ```text
//! mg-verify <artefact> --trust-anchor <path> [--trust-anchor <path>…] [--quiet]
//! mg-verify --trust-anchor <path> --run-service <service options>…
//! ```
//!
//! Trust anchors are mandatory and explicit. There is no default anchor location
//! and no "no anchors means accept anything" path: absence is a refusal.
//! Distribution, rotation and revocation of anchors are RF-SC-8.
//!
//! On success it prints one machine-readable line to **stdout** and exits 0:
//!
//! ```text
//! ok artefact=<name> digest=<hex> key=<id> alg=<alg> components=<n>
//! ```
//!
//! Everything else goes to stderr with a non-zero exit. There is no soft pass.
//!
//! `--run-service` is verify-before-run for a release image, whose runtime stage is
//! distroless and has no shell to run a script in (deploy/images/service.Dockerfile).
//! It verifies `<MIDGARD_APP_DIR, default /app>/<MIDGARD_SERVICE>` exactly as above
//! and, only when that passes, replaces itself with the binary, handing it every
//! argument after `--run-service`. The binary is read once, and on Linux what runs is
//! those verified bytes, from a sealed in-memory file: the path is never opened again,
//! so a binary swapped in after the check is not the one started. Elsewhere the path
//! is executed, as before. Those arguments are the service's, never this
//! tool's: the trust anchor is the one on the image's own command line, and nothing
//! in the environment or the container's arguments can name another. A service
//! started with no options is refused rather than started on a default address.
//! [AC-B-1] [NFR-PQ-3] [MG-SEC-06] [MG-SEC-14] [VLD-ANB-MIDGARD-0068]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use attest::{verify_artefact, verify_artefact_bytes, TrustAnchors};

const USAGE: &str = "\
usage:
  mg-verify <artefact> --trust-anchor <path> [--trust-anchor <path>...] [--quiet]
  mg-verify --trust-anchor <path> [--quiet] --run-service [--] <service options>...

Verifies, and refuses unless every one of these holds:
  * <artefact>.sig, <artefact>.sbom.cdx.json and <artefact>.att all exist
  * the .sig envelope parses and carries the release domain label
  * the algorithm is approved for release signing
  * the signing key id is present in the trust anchor set
  * the anchor and the signature agree on the algorithm
  * the signature verifies over the artefact, SBOM and attestation digests
    recomputed from disk
  * the SBOM is well-formed CycloneDX 1.5 with at least one component, each
    carrying a purl

Trust anchors are lines of `<alg> <key-id> <hex>`, as written by
`mg-sign keygen --export-verifying-key`. They are mandatory: with none supplied,
verification refuses.

--run-service verifies $MIDGARD_APP_DIR/$MIDGARD_SERVICE ($MIDGARD_APP_DIR is /app
when unset) the same way and, only if it verifies, runs it in place of mg-verify
with every argument after --run-service (a leading `--` is dropped). It takes no
artefact path, and a service given no options is refused (exit 2). A binary that
does not verify is refused (exit 1) and never started.
";

/// See `mg-sign`: ML-DSA verification holds large fixed-size arrays by value, so
/// an unoptimised build overflows the default main-thread stack. A generous
/// worker stack makes the tool behave the same in debug and release.
const STACK_SIZE: usize = 64 * 1024 * 1024;

fn main() -> ExitCode {
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(verify_main)
        .expect("spawn worker thread")
        .join()
        .unwrap_or(ExitCode::FAILURE)
}

fn verify_main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // Help is its own invocation. It exits 0, so it must never be reachable
    // from a command line that also names an artefact: a caller that checks only
    // the exit code would read that as "verified".
    if matches!(argv.as_slice(), [only] if is_help(only)) {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if let Some(at) = argv.iter().position(|a| a == RUN_SERVICE) {
        return run_service(&argv[..at], &argv[at + 1..]);
    }
    match run(&argv) {
        Ok(line) => {
            // stdout carries ONLY the verdict, so callers can parse it.
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(failure) => failure.report(),
    }
}

#[derive(Debug)]
enum Failure {
    Usage(String),
    Refused(String),
}

impl Failure {
    /// Print the failure to stderr and give its exit code: 2 for a usage error, 1
    /// for a refusal.
    fn report(self) -> ExitCode {
        match self {
            Failure::Usage(m) => {
                eprintln!("mg-verify: {m}\n");
                eprint!("{USAGE}");
                ExitCode::from(2)
            }
            Failure::Refused(m) => {
                eprintln!("mg-verify: REFUSED — {m}");
                ExitCode::FAILURE
            }
        }
    }
}

const RUN_SERVICE: &str = "--run-service";

fn is_help(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
}

/// The tool's own arguments: the artefact (if named), the anchor files and `--quiet`.
struct Options {
    artefact: Option<PathBuf>,
    anchor_paths: Vec<PathBuf>,
    quiet: bool,
}

fn run(argv: &[String]) -> Result<String, Failure> {
    let opts = parse(argv)?;
    let artefact = opts
        .artefact
        .ok_or_else(|| Failure::Usage("no artefact given".into()))?;
    verify(&artefact, None, &opts.anchor_paths, opts.quiet)
}

fn parse(argv: &[String]) -> Result<Options, Failure> {
    let mut artefact: Option<PathBuf> = None;
    let mut anchor_paths: Vec<PathBuf> = Vec::new();
    let mut quiet = false;

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--trust-anchor" => {
                let v = argv
                    .get(i + 1)
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| Failure::Usage("--trust-anchor requires a path".into()))?;
                anchor_paths.push(PathBuf::from(v));
                i += 2;
            }
            "--quiet" => {
                quiet = true;
                i += 1;
            }
            help if is_help(help) => {
                return Err(Failure::Usage(format!(
                    "{help} must be the only argument — it does not verify anything"
                )))
            }
            other if other.starts_with("--") => {
                return Err(Failure::Usage(format!("unknown flag {other}")))
            }
            other => {
                if artefact.is_some() {
                    return Err(Failure::Usage(
                        "verify takes exactly one artefact path".into(),
                    ));
                }
                artefact = Some(PathBuf::from(other));
                i += 1;
            }
        }
    }

    Ok(Options {
        artefact,
        anchor_paths,
        quiet,
    })
}

/// Verify one artefact against the anchor files, giving its verdict line. With
/// `bytes`, those are the artefact (already read), and the file at `artefact` is not
/// opened; only its sidecars are read.
fn verify(
    artefact: &Path,
    bytes: Option<&[u8]>,
    anchor_paths: &[PathBuf],
    quiet: bool,
) -> Result<String, Failure> {
    if anchor_paths.is_empty() {
        // Explicit, because "I forgot the anchors" and "this artefact is fine"
        // must never look the same to an operator or a script.
        return Err(Failure::Usage(
            "no --trust-anchor given: verification requires an explicit trust anchor set".into(),
        ));
    }

    let anchors = load_anchors(anchor_paths)?;
    let verdict = match bytes {
        Some(bytes) => verify_artefact_bytes(bytes, artefact, &anchors),
        None => verify_artefact(artefact, &anchors),
    }
    .map_err(|e| Failure::Refused(format!("{} — {e}", artefact.display())))?;

    if !quiet {
        eprintln!(
            "mg-verify: {} verified against {} trust anchor(s)",
            artefact.display(),
            anchor_paths.len()
        );
    }
    Ok(verdict.line())
}

/// What `--run-service` starts once the binary has verified.
struct ServicePlan<'a> {
    bin: PathBuf,
    name: String,
    args: &'a [String],
    opts: Options,
}

/// `--run-service`: verify-before-run, the release image's entrypoint. `own` is
/// what came before `--run-service` (this tool's options), `service` what came
/// after (the service's). The messages are the ones deploy/images/build.sh looks
/// for when it starts each binary through its image. [AC-B-1] [NFR-PQ-3]
fn run_service(own: &[String], service: &[String]) -> ExitCode {
    let plan = match service_plan(own, service) {
        Ok(plan) => plan,
        Err(failure) => return failure.report(),
    };
    let name = &plan.name;
    // Safe by default: a service started with no options is refused, never started
    // on a default address. The gateway in particular needs its state directory
    // (--state-dir); its development mode (--dev) listens on loopback only, so it is
    // of no use in an image. [MG-SEC-06]
    if plan.args.is_empty() {
        eprintln!(
            "::fatal:: {name} was started with no options — refusing to start. Pass the \
             service's own options as the container's arguments (for http-gateway: \
             --state-dir DIR --bind 0.0.0.0:7710 --allow-non-loopback ...)."
        );
        return ExitCode::from(2);
    }

    let anchors: Vec<String> = plan
        .opts
        .anchor_paths
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    println!(
        "verify-before-run: checking {} against {}",
        plan.bin.display(),
        anchors.join(" ")
    );
    // Read once: these bytes are what is verified, and what is started.
    let bytes = match std::fs::read(&plan.bin) {
        Ok(bytes) => bytes,
        Err(e) => {
            let code =
                Failure::Refused(format!("cannot read {}: {e}", plan.bin.display())).report();
            eprintln!(
                "::fatal:: {name} failed PQ signature/SBOM/provenance verification — refusing to start"
            );
            return code;
        }
    };
    match verify(
        &plan.bin,
        Some(&bytes),
        &plan.opts.anchor_paths,
        plan.opts.quiet,
    ) {
        Ok(line) => println!("{line}"),
        Err(failure) => {
            let code = failure.report();
            eprintln!(
                "::fatal:: {name} failed PQ signature/SBOM/provenance verification — refusing to start"
            );
            return code;
        }
    }
    println!("verify-before-run: OK — starting {name}");
    // Nothing printed above may be lost when the process image is replaced.
    let _ = std::io::stdout().flush();
    exec(&bytes, &plan.bin, name, plan.args)
}

/// Work out what `--run-service` would start, refusing anything it must not.
fn service_plan<'a>(own: &[String], service: &'a [String]) -> Result<ServicePlan<'a>, Failure> {
    let opts = parse(own)?;
    if opts.artefact.is_some() {
        return Err(Failure::Usage(format!(
            "{RUN_SERVICE} verifies $MIDGARD_SERVICE; it takes no artefact path"
        )));
    }
    // A leading `--` separates explicitly. Without it the service's arguments are its
    // options, so a bare path straight after --run-service is a mistaken attempt to
    // name an artefact, not something to hand to the service.
    let args = match service {
        [sep, rest @ ..] if sep == "--" => rest,
        [first, ..] if !first.starts_with('-') => {
            return Err(Failure::Usage(format!(
                "{RUN_SERVICE} verifies $MIDGARD_SERVICE; it takes no artefact path \
                 (the service's options follow it, or follow `--`)"
            )))
        }
        all => all,
    };
    let name = std::env::var("MIDGARD_SERVICE").unwrap_or_default();
    if name.is_empty() {
        return Err(Failure::Usage(
            "MIDGARD_SERVICE (the service binary name) must be set".into(),
        ));
    }
    // A name, not a path: the binary is always one the image's app directory holds.
    if name.contains('/') || name == "." || name == ".." {
        return Err(Failure::Usage(format!(
            "MIDGARD_SERVICE must name a binary, not a path: {name:?}"
        )));
    }
    let app_dir = std::env::var_os("MIDGARD_APP_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/app"));
    Ok(ServicePlan {
        bin: app_dir.join(&name),
        name,
        args,
        opts,
    })
}

/// Become the verified service: on Linux, the verified `bytes` themselves, never
/// `bin` again. Returns only if it could not be started.
#[cfg(target_os = "linux")]
fn exec(bytes: &[u8], _bin: &Path, name: &str, args: &[String]) -> ExitCode {
    let err = exec_bytes(bytes, name, args);
    eprintln!("::fatal:: {name} verified but could not be started: {err}");
    ExitCode::FAILURE
}

/// Execute `bytes` as `name` with `args`: copied into an anonymous in-memory file
/// (`memfd_create`), sealed so nothing can change, grow or shrink it, reopened
/// read-only (so no descriptor that could write it is open when it is executed), and
/// executed through `/proc/self/fd`. Both descriptors close on exec, so the service
/// inherits neither. Returns only the reason it could not. [AC-B-1] [NFR-PQ-3]
/// [VLD-ANB-MIDGARD-0068]
#[cfg(target_os = "linux")]
fn exec_bytes(bytes: &[u8], name: &str, args: &[String]) -> std::io::Error {
    use std::io::{Error, ErrorKind};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::process::CommandExt;

    let Ok(label) = std::ffi::CString::new(name) else {
        return Error::new(ErrorKind::InvalidInput, "the service name holds a NUL byte");
    };
    // SAFETY: `label` is a NUL-terminated string that outlives the call, and the flags
    // are valid; the call returns a new descriptor or -1.
    let fd =
        unsafe { libc::memfd_create(label.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Error::last_os_error();
    }
    // SAFETY: `fd` was just created and nothing else owns it.
    let mut memfd = unsafe { std::fs::File::from_raw_fd(fd) };
    if let Err(e) = memfd.write_all(bytes) {
        return e;
    }
    let seals = libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
    // SAFETY: `memfd` is an open memfd created with MFD_ALLOW_SEALING.
    if unsafe { libc::fcntl(memfd.as_raw_fd(), libc::F_ADD_SEALS, seals) } < 0 {
        return Error::last_os_error();
    }
    let sealed = match std::fs::File::open(format!("/proc/self/fd/{}", memfd.as_raw_fd())) {
        Ok(file) => file,
        Err(e) => return e,
    };
    drop(memfd);
    std::process::Command::new(format!("/proc/self/fd/{}", sealed.as_raw_fd()))
        .arg0(name)
        .args(args)
        .exec()
}

/// Become the verified service. Without `memfd_create` the path is executed, as it
/// always was. Returns only if it could not be started.
#[cfg(all(unix, not(target_os = "linux")))]
fn exec(_bytes: &[u8], bin: &Path, name: &str, args: &[String]) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(bin).args(args).exec();
    eprintln!("::fatal:: {name} verified but could not be started: {err}");
    ExitCode::FAILURE
}

#[cfg(not(unix))]
fn exec(_bytes: &[u8], _bin: &Path, name: &str, _args: &[String]) -> ExitCode {
    eprintln!("::fatal:: {name} verified, but {RUN_SERVICE} starts services on Unix only");
    ExitCode::FAILURE
}

/// Load and merge every anchor file. A malformed file is a refusal, never a
/// silently skipped anchor — an operator who mistypes an anchor path must not
/// end up verifying against a smaller set than they intended.
fn load_anchors(paths: &[PathBuf]) -> Result<TrustAnchors, Failure> {
    let mut merged = TrustAnchors::new();
    for p in paths {
        let text = std::fs::read_to_string(p).map_err(|e| {
            Failure::Usage(format!("cannot read trust anchors {}: {e}", p.display()))
        })?;
        let parsed = TrustAnchors::parse(&text)
            .map_err(|e| Failure::Usage(format!("{}: {e}", p.display())))?;
        merged
            .merge(parsed)
            .map_err(|e| Failure::Usage(format!("{}: {e}", p.display())))?;
    }
    if merged.is_empty() {
        return Err(Failure::Usage(format!(
            "{} anchor file(s) contained no anchors",
            paths.len()
        )));
    }
    Ok(merged)
}

/// Where an artefact's sidecars are expected, for diagnostics.
#[allow(dead_code)]
fn sidecars(artefact: &Path) -> [PathBuf; 3] {
    [
        attest::sig_path(artefact),
        attest::sbom::sbom_path(artefact),
        attest::att_path(artefact),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_loading_refuses_a_missing_file() {
        let missing = std::env::temp_dir().join("mg-verify-no-such-anchors");
        let _ = std::fs::remove_file(&missing);
        assert!(matches!(load_anchors(&[missing]), Err(Failure::Usage(_))));
    }

    #[test]
    fn anchor_loading_refuses_a_malformed_file() {
        let p = std::env::temp_dir().join("mg-verify-bad-anchors");
        std::fs::write(&p, "this is not an anchor line").unwrap();
        assert!(matches!(load_anchors(&[p.clone()]), Err(Failure::Usage(_))));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn anchor_loading_refuses_an_empty_file() {
        let p = std::env::temp_dir().join("mg-verify-empty-anchors");
        std::fs::write(&p, "# only a comment\n\n").unwrap();
        assert!(matches!(load_anchors(&[p.clone()]), Err(Failure::Usage(_))));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn anchor_loading_merges_several_files() {
        let a = std::env::temp_dir().join("mg-verify-anchors-a");
        let b = std::env::temp_dir().join("mg-verify-anchors-b");
        std::fs::write(&a, "ML-DSA-87 key-a aabb\n").unwrap();
        std::fs::write(&b, "ML-DSA-65 key-b ccdd\n").unwrap();
        let merged = load_anchors(&[a.clone(), b.clone()]).expect("merges");
        assert!(merged.get("key-a").is_some());
        assert!(merged.get("key-b").is_some());
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn anchor_loading_refuses_one_key_id_anchored_to_two_keys() {
        let a = std::env::temp_dir().join("mg-verify-anchors-conflict-a");
        let b = std::env::temp_dir().join("mg-verify-anchors-conflict-b");
        std::fs::write(&a, "ML-DSA-87 release-key-1 aabb\n").unwrap();
        std::fs::write(&b, "ML-DSA-87 release-key-1 ccdd\n").unwrap();
        // Which key is trusted must not depend on the order the files were named.
        assert!(matches!(
            load_anchors(&[a.clone(), b.clone()]),
            Err(Failure::Usage(_))
        ));
        // The same anchor named twice is not a conflict.
        assert!(load_anchors(&[a.clone(), a.clone()]).is_ok());
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn help_alongside_other_arguments_is_a_usage_error() {
        let argv =
            |items: &[&str]| -> Vec<String> { items.iter().map(|s| s.to_string()).collect() };
        for bad in [
            vec!["artefact", "--trust-anchor", "anchors", "--help"],
            vec!["--help", "artefact", "--trust-anchor", "anchors"],
            vec!["artefact", "-h"],
        ] {
            assert!(
                matches!(run(&argv(&bad)), Err(Failure::Usage(_))),
                "{bad:?} must not be treated as a request for help"
            );
        }
    }

    #[test]
    fn sidecar_paths_are_the_three_the_verifier_requires() {
        let s = sidecars(Path::new("target/release/mg-sbom"));
        assert!(s[0].to_string_lossy().ends_with(".sig"));
        assert!(s[1].to_string_lossy().ends_with(".sbom.cdx.json"));
        assert!(s[2].to_string_lossy().ends_with(".att"));
    }
}
