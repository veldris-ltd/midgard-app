//! mg-verify — verify an artefact's ML-DSA signature, SBOM, and provenance
//! before publish or deploy. FAILS CLOSED. [NFR-PQ-3] [AC-B-1] [AC-B-4]
//!
//! This binary is argument handling and an exit code over
//! [`attest::verify_artefact`], the file-level verifier. It runs in CI (stage 7,
//! pre-publish) and in every image's entrypoint before the service starts.
//! Bifrost's pre-deploy check (AC-B-1) and Mimir's model load (AC-MS-2) verify
//! their own manifests rather than files, so they do not call this — but all of
//! them reach the same signature decision, `attest::accept::accept`: one
//! implementation of "reject unless proven".
//!
//! ```text
//! mg-verify <artefact> --trust-anchor <path> [--trust-anchor <path>…] [--quiet]
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

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use attest::{verify_artefact, TrustAnchors};

const USAGE: &str = "\
usage:
  mg-verify <artefact> --trust-anchor <path> [--trust-anchor <path>...] [--quiet]

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
    match run(&argv) {
        Ok(line) => {
            // stdout carries ONLY the verdict, so callers can parse it.
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(Failure::Usage(m)) => {
            eprintln!("mg-verify: {m}\n");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Refused(m)) => {
            eprintln!("mg-verify: REFUSED — {m}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
enum Failure {
    Usage(String),
    Refused(String),
}

fn is_help(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
}

fn run(argv: &[String]) -> Result<String, Failure> {
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

    let artefact = artefact.ok_or_else(|| Failure::Usage("no artefact given".into()))?;
    if anchor_paths.is_empty() {
        // Explicit, because "I forgot the anchors" and "this artefact is fine"
        // must never look the same to an operator or a script.
        return Err(Failure::Usage(
            "no --trust-anchor given: verification requires an explicit trust anchor set".into(),
        ));
    }

    let anchors = load_anchors(&anchor_paths)?;
    let verdict = verify_artefact(&artefact, &anchors)
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
