//! mg-sign — PQ-sign a release artefact with ML-DSA. [NFR-PQ-3]
//!
//! Emits a detached signature over a domain-separated preimage binding the
//! artefact, its CycloneDX SBOM, and its in-toto provenance attestation, so no
//! part of the release bundle can be swapped under a valid signature. Keys are
//! held by a KMS/HSM custodian and are NEVER read from the repo, the image, or a
//! default path. [STD:OWASP ASVS V14.3]
//!
//! The format, the custody seam, and the verification counterpart all live in
//! `attest`, so this binary is only argument handling and provenance
//! gathering. `mg-verify` (RF-SC-2) wraps the same library, which is what makes
//! "acceptable" mean one thing across CI, Bifrost pre-deploy, and model load.
//!
//! ```text
//! mg-sign keygen --keystore <path> [--key-id <id>] [--alg <name>]
//!                [--export-verifying-key <path>]
//! mg-sign sign   --keystore <path> | --key-file <path>
//!                [--sbom <path>] [--source-revision <rev>]
//!                [--rustc-version <v>] [--toolchain-sha256 <hex>]
//!                [--repro <path>] <artefact>
//! ```
//!
//! Exit codes: `0` signed, `2` usage error, `1` everything else. There is no
//! success path that did not produce a valid signature.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use attest::{
    keystore::DevKeyFile, sign_artefact, FileSigner, Keystore, KmsSigner, Provenance, ReproEntry,
    SignError,
};
use pqc_agility::SigId;

const USAGE: &str = "\
usage:
  mg-sign keygen --keystore <path> [--key-id <id>] [--alg <name>]
                 [--export-verifying-key <path>]
  mg-sign sign   (--keystore <path> | --key-file <path>) [--sbom <path>]
                 [--source-revision <rev>] [--rustc-version <v>]
                 [--toolchain-sha256 <hex>] [--repro <path>] <artefact>

Key sources are explicit and mandatory. mg-sign never searches for a key.
  --keystore   a sealed keystore (mg-sign keygen). The signing key is unsealed
               inside the KMS custodian; this is the production path.
  --key-file   a plaintext development key. REFUSED unless
               MIDGARD_ALLOW_FILE_SIGNING_KEY=1.
";

/// ML-KEM-1024 and ML-DSA-87 keygen hold large fixed-size arrays by value. An
/// optimised build elides them, but an unoptimised one (`cargo run` without
/// `--release`) blows the default 8 MiB main-thread stack and dies with a bare
/// "stack overflow" that looks like a crash rather than a build-profile problem.
/// Running the work on a thread with a generous stack makes the tool behave the
/// same either way.
const STACK_SIZE: usize = 64 * 1024 * 1024;

fn main() -> ExitCode {
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(dispatch)
        .expect("spawn worker thread")
        .join()
        .unwrap_or(ExitCode::FAILURE)
}

fn dispatch() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };

    match cmd {
        "keygen" => run(keygen(&args[1..])),
        "sign" => run(sign(&args[1..])),
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("mg-sign: unknown command {other:?}\n");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Map a command result onto an exit code. Every error is a non-zero exit with a
/// reason on stderr — never a soft pass.
fn run(r: Result<(), Failure>) -> ExitCode {
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(m)) => {
            eprintln!("mg-sign: {m}\n");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Fatal(m)) => {
            eprintln!("mg-sign: {m}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
enum Failure {
    Usage(String),
    Fatal(String),
}

impl From<SignError> for Failure {
    fn from(e: SignError) -> Self {
        Failure::Fatal(e.to_string())
    }
}

// --- argument parsing --------------------------------------------------------

/// A minimal flag parser. Deliberately strict: an unknown flag or a flag with no
/// value is a usage error, never silently ignored — a mistyped `--keystore` must
/// not fall through to some other key source.
struct Args {
    flags: Vec<(String, String)>,
    positional: Vec<String>,
}

impl Args {
    fn parse(argv: &[String], known: &[&str]) -> Result<Self, Failure> {
        let mut flags = Vec::new();
        let mut positional = Vec::new();
        let mut i = 0;
        while i < argv.len() {
            let a = &argv[i];
            if let Some(name) = a.strip_prefix("--") {
                if !known.contains(&name) {
                    return Err(Failure::Usage(format!("unknown flag --{name}")));
                }
                let value = argv
                    .get(i + 1)
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| Failure::Usage(format!("--{name} requires a value")))?;
                flags.push((name.to_string(), value.clone()));
                i += 2;
            } else {
                positional.push(a.clone());
                i += 1;
            }
        }
        Ok(Self { flags, positional })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

fn parse_alg(s: &str) -> Result<SigId, Failure> {
    [SigId::MlDsa87, SigId::MlDsa65, SigId::MlDsa44]
        .into_iter()
        .find(|a| a.as_str() == s)
        .ok_or_else(|| {
            Failure::Usage(format!(
                "unknown signing algorithm {s:?} (expected ML-DSA-87, ML-DSA-65 or ML-DSA-44)"
            ))
        })
}

// --- keygen ------------------------------------------------------------------

fn keygen(argv: &[String]) -> Result<(), Failure> {
    let args = Args::parse(argv, &["keystore", "key-id", "alg", "export-verifying-key"])?;
    let keystore_path = args
        .get("keystore")
        .ok_or_else(|| Failure::Usage("keygen requires --keystore <path>".into()))?;
    let key_id = args.get("key-id").unwrap_or("release-key-1");
    let alg = match args.get("alg") {
        Some(s) => parse_alg(s)?,
        None => kms::RELEASE_SIG_ALG,
    };

    let mut rng = pqc_agility::OsRng;
    let ks = Keystore::generate(key_id, alg, &mut rng)?;
    ks.write(Path::new(keystore_path))?;

    if let Some(p) = args.get("export-verifying-key") {
        std::fs::write(p, ks.export_verifying_key())
            .map_err(|e| Failure::Fatal(format!("cannot write verifying key to {p}: {e}")))?;
        eprintln!("mg-sign: wrote trust anchor {p}");
    }

    eprintln!(
        "mg-sign: wrote keystore {keystore_path} (key id {key_id}, {})",
        alg.as_str()
    );
    eprintln!("  the signing key is sealed under an ML-KEM-1024 root; protect this file as a custody device.");
    Ok(())
}

// --- sign --------------------------------------------------------------------

fn sign(argv: &[String]) -> Result<(), Failure> {
    let args = Args::parse(
        argv,
        &[
            "keystore",
            "key-file",
            "sbom",
            "source-revision",
            "rustc-version",
            "toolchain-sha256",
            "repro",
        ],
    )?;

    let artefact = match args.positional.as_slice() {
        [one] => PathBuf::from(one),
        [] => return Err(Failure::Usage("sign requires an <artefact> path".into())),
        many => {
            return Err(Failure::Usage(format!(
                "sign takes exactly one artefact, got {}",
                many.len()
            )))
        }
    };

    // Exactly one key source, named explicitly. Neither is a default.
    let signer: Box<dyn attest::SigningBackend> = match (args.get("keystore"), args.get("key-file"))
    {
        (Some(_), Some(_)) => {
            return Err(Failure::Usage(
                "--keystore and --key-file are mutually exclusive".into(),
            ))
        }
        (Some(p), None) => {
            let ks = Keystore::read(Path::new(p))?;
            Box::new(KmsSigner::new(ks.into_signing_root()?))
        }
        (None, Some(p)) => {
            // Check the opt-in BEFORE touching the key path. Reading first
            // and refusing second would report "cannot read key" for an
            // operator whose real problem is that this mode is not enabled,
            // and would touch key material we were never permitted to use.
            if !attest::backend::file_signing_permitted() {
                return Err(SignError::FileSigningNotPermitted.into());
            }
            let dev = DevKeyFile::read(Path::new(p))?;
            Box::new(FileSigner::new(
                pqc_agility::backend::fips::provider(),
                &dev.key_id,
                dev.verifying_key,
                dev.signing_key,
            )?)
        }
        (None, None) => {
            return Err(Failure::Usage(
                "no key source: pass --keystore <path> or --key-file <path>".into(),
            ))
        }
    };

    let prov = gather_provenance(&args)?;
    let sbom_override = args.get("sbom").map(PathBuf::from);
    let out = sign_artefact(&artefact, sbom_override.as_deref(), signer.as_ref(), &prov)?;

    eprintln!(
        "mg-sign: signed {} ({}, key {}, {} SBOM components)",
        artefact.display(),
        out.alg.as_str(),
        out.key_id,
        out.components
    );
    eprintln!("  {}", out.sig_path.display());
    eprintln!("  {}", out.att_path.display());
    Ok(())
}

/// Assemble the build facts recorded in the attestation.
///
/// Every field is explicit-first: a flag wins, then a CI environment variable,
/// then a local discovery step. If a fact cannot be established, signing refuses
/// rather than recording a placeholder — provenance that says "unknown" is
/// provenance nobody can rely on.
fn gather_provenance(args: &Args) -> Result<Provenance, Failure> {
    let source_revision = resolve_source_revision(
        args.get("source-revision"),
        std::env::var("MIDGARD_SOURCE_REVISION").ok(),
        std::env::var("GITHUB_SHA").ok(),
        || capture("git", &["rev-parse", "HEAD"]),
    )
    .map_err(Failure::Usage)?;

    let rustc_version = match args.get("rustc-version") {
        Some(v) => v.to_string(),
        None => capture("rustc", &["--version"]).ok_or_else(|| {
            Failure::Usage("cannot determine the rustc version: pass --rustc-version <v>".into())
        })?,
    };

    // The toolchain hash pins *what built this*. Prefer the workspace's pinned
    // toolchain file; fall back to the compiler's own version string.
    let toolchain_sha256 = match args.get("toolchain-sha256") {
        Some(v) => v.to_string(),
        None => {
            let material = std::fs::read("rust-toolchain.toml")
                .unwrap_or_else(|_| rustc_version.as_bytes().to_vec());
            attest::hex::hex(&pqc_agility::hash::sha256(&material))
        }
    };

    let repro = match args.get("repro") {
        Some(p) => parse_repro(Path::new(p))?,
        None => Vec::new(),
    };

    Ok(Provenance {
        source_revision,
        toolchain_sha256,
        rustc_version,
        repro,
    })
}

/// The source revision the attestation records: the flag, then
/// `MIDGARD_SOURCE_REVISION`, then `GITHUB_SHA`, then `git rev-parse HEAD`. A blank
/// value is no value: an environment variable that is set but empty falls through to
/// the next source, and a blank flag is refused. An attestation never records an
/// empty revision. [AC-B-4] [NFR-PQ-3]
fn resolve_source_revision(
    flag: Option<&str>,
    midgard: Option<String>,
    github: Option<String>,
    git: impl FnOnce() -> Option<String>,
) -> Result<String, String> {
    if let Some(v) = flag {
        let v = v.trim();
        if v.is_empty() {
            return Err("--source-revision is empty".into());
        }
        return Ok(v.to_string());
    }
    let present = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    present(midgard)
        .or_else(|| present(github))
        .or_else(|| present(git()))
        .ok_or_else(|| {
            "cannot determine the source revision: pass --source-revision <rev> \
             or set MIDGARD_SOURCE_REVISION"
                .to_string()
        })
}

#[cfg(test)]
mod source_revision_tests {
    use super::resolve_source_revision;

    fn no_git() -> Option<String> {
        None
    }

    #[test]
    fn the_flag_wins_then_the_environment_then_git() {
        let rev = |flag, midgard: Option<&str>, github: Option<&str>, git: Option<&str>| {
            resolve_source_revision(
                flag,
                midgard.map(String::from),
                github.map(String::from),
                || git.map(String::from),
            )
        };
        assert_eq!(
            rev(Some("f1"), Some("m"), Some("g"), Some("h")).unwrap(),
            "f1"
        );
        assert_eq!(rev(None, Some("m"), Some("g"), Some("h")).unwrap(), "m");
        assert_eq!(rev(None, None, Some("g"), Some("h")).unwrap(), "g");
        assert_eq!(rev(None, None, None, Some("h\n")).unwrap(), "h");
    }

    #[test]
    fn a_blank_revision_is_never_recorded() {
        // Set but empty falls through to the next source.
        assert_eq!(
            resolve_source_revision(None, Some(String::new()), Some("g".into()), no_git).unwrap(),
            "g"
        );
        assert_eq!(
            resolve_source_revision(None, Some("  ".into()), None, || Some("h".into())).unwrap(),
            "h"
        );
        // Blank everywhere is a refusal, not an empty attestation.
        assert!(
            resolve_source_revision(None, Some(String::new()), Some(" ".into()), || Some(
                String::new()
            ))
            .is_err()
        );
        // An explicitly blank flag is refused rather than skipped.
        assert!(resolve_source_revision(Some(" "), Some("m".into()), None, no_git).is_err());
    }
}

/// Run a command and return its trimmed stdout, or `None` if it is unavailable
/// or failed. Used only for provenance discovery, never for a security decision.
fn capture(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Read `mg-repro`'s report (RF-SC-3) into attestation entries.
///
/// The input is produced by our own tool, so this is a targeted extractor for
/// `"package"` / `"sha256"` pairs rather than a general JSON parser — the
/// attestation's integrity comes from being bound into the signature, not from
/// how it was assembled.
fn parse_repro(path: &Path) -> Result<Vec<ReproEntry>, Failure> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Failure::Fatal(format!("cannot read {}: {e}", path.display())))?;

    let field = |s: &str, name: &str| -> Option<String> {
        let key = format!("\"{name}\"");
        let at = s.find(&key)? + key.len();
        let rest = &s[at..];
        let start = rest.find('"')? + 1;
        let end = rest[start..].find('"')? + start;
        Some(rest[start..end].to_string())
    };

    let mut out = Vec::new();
    for chunk in text.split('{').skip(1) {
        let chunk = chunk.split('}').next().unwrap_or(chunk);
        if let (Some(package), Some(sha256)) = (field(chunk, "package"), field(chunk, "sha256")) {
            out.push(ReproEntry { package, sha256 });
        }
    }
    if out.is_empty() {
        return Err(Failure::Fatal(format!(
            "{} contained no package/sha256 entries",
            path.display()
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rejects_unknown_flags_rather_than_ignoring_them() {
        let e = Args::parse(&argv(&["--keystoer", "x"]), &["keystore"]);
        assert!(
            matches!(e, Err(Failure::Usage(_))),
            "a typo must not fall through"
        );
    }

    #[test]
    fn rejects_a_flag_with_no_value() {
        let e = Args::parse(&argv(&["--keystore"]), &["keystore"]);
        assert!(matches!(e, Err(Failure::Usage(_))));
        let e = Args::parse(&argv(&["--keystore", "--sbom", "s"]), &["keystore", "sbom"]);
        assert!(matches!(e, Err(Failure::Usage(_))));
    }

    #[test]
    fn separates_flags_from_the_artefact() {
        let a = Args::parse(
            &argv(&["--keystore", "k", "target/release/mg-sbom"]),
            &["keystore"],
        )
        .expect("parses");
        assert_eq!(a.get("keystore"), Some("k"));
        assert_eq!(a.positional, vec!["target/release/mg-sbom".to_string()]);
    }

    #[test]
    fn sign_requires_a_key_source() {
        let e = sign(&argv(&["artefact"])).expect_err("must refuse");
        match e {
            Failure::Usage(m) => assert!(m.contains("no key source"), "{m}"),
            Failure::Fatal(m) => panic!("expected a usage error, got: {m}"),
        }
    }

    #[test]
    fn key_file_is_refused_before_the_key_path_is_touched() {
        // The refusal must not depend on the file existing: an operator without
        // the opt-in should be told that, not that their path is wrong.
        // [AC-RF-SC-1d]
        let restore = std::env::var(attest::backend::FILE_SIGNING_ENV).ok();
        std::env::remove_var(attest::backend::FILE_SIGNING_ENV);

        let missing = std::env::temp_dir().join("mg-sign-no-such-key-file");
        let _ = std::fs::remove_file(&missing);
        let e = sign(&argv(&[
            "--key-file",
            missing.to_str().unwrap(),
            "some-artefact",
        ]))
        .expect_err("must refuse");
        match e {
            Failure::Fatal(m) => assert!(
                m.contains("MIDGARD_ALLOW_FILE_SIGNING_KEY"),
                "expected the opt-in refusal, got: {m}"
            ),
            Failure::Usage(m) => panic!("expected the opt-in refusal, got usage: {m}"),
        }

        if let Some(v) = restore {
            std::env::set_var(attest::backend::FILE_SIGNING_ENV, v);
        }
    }

    #[test]
    fn sign_refuses_two_key_sources() {
        let e =
            sign(&argv(&["--keystore", "a", "--key-file", "b", "art"])).expect_err("must refuse");
        match e {
            Failure::Usage(m) => assert!(m.contains("mutually exclusive"), "{m}"),
            Failure::Fatal(m) => panic!("expected a usage error, got: {m}"),
        }
    }

    #[test]
    fn sign_requires_exactly_one_artefact() {
        for bad in [vec!["--keystore", "k"], vec!["--keystore", "k", "a", "b"]] {
            assert!(matches!(sign(&argv(&bad)), Err(Failure::Usage(_))));
        }
    }

    #[test]
    fn parses_known_algorithms_and_rejects_others() {
        assert_eq!(parse_alg("ML-DSA-87").unwrap(), SigId::MlDsa87);
        assert_eq!(parse_alg("ML-DSA-65").unwrap(), SigId::MlDsa65);
        assert!(
            parse_alg("SLH-DSA-SHA2-128s").is_err(),
            "not for release signing"
        );
        assert!(parse_alg("Ed25519").is_err(), "not post-quantum");
    }

    #[test]
    fn parses_a_repro_report() {
        let dir = std::env::temp_dir().join("mg-sign-repro-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("repro.json");
        std::fs::write(
            &p,
            r#"{"packages":[
                 {"package":"mg-sbom","sha256":"aa"},
                 {"package":"mg-sign","sha256":"bb"}]}"#,
        )
        .unwrap();

        let entries = parse_repro(&p).expect("parses");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].package, "mg-sbom");
        assert_eq!(entries[1].sha256, "bb");

        std::fs::write(&p, "{}").unwrap();
        assert!(parse_repro(&p).is_err(), "an empty report is an error");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
