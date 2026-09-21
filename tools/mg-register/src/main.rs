//! `mg-register` — hand a gate-verified artefact to Bifrost.
//!
//! This is the last stage of the release contract. `just gate <pkg>` proves the
//! artefact reproducible, SBOMs it, scans it, ML-DSA-signs it and verifies that
//! exact binary; this then admits it to the Bifrost artefact registry, which is
//! what makes it eligible for ring rollout and for air-gap bundling.
//!
//! The registry is the gate, not this tool: [`DurableArtefactRegistry::ingest`]
//! verifies the artefact and stores it **only** if it passes, so a tampered or
//! mis-signed artefact never reaches disk and never reaches a fleet.
//! [FR-B-1] [FR-B-4] [AC-B-1] [NFR-PQ-3]
//!
//! Usage:
//!   mg-register --registry <dir> --artefact <path> --sbom <path>
//!               --id <id> --version <ver> [--component <kind>]
//!               --keystore <path>
//!
//! Every input is explicit. There is no search path and no default location: a
//! release that cannot say which registry it is publishing to, under which key,
//! must fail rather than guess. Mirrors `just verify`'s refusal to run without
//! `MIDGARD_TRUST_ANCHORS`.

use std::path::Path;
use std::process::exit;

use bifrost_artefact::{build_and_sign, DurableArtefactRegistry};
use kms::SigningRoot;

const USAGE: &str = "\
mg-register — admit a gate-verified artefact to the Bifrost artefact registry

USAGE:
  mg-register --registry <dir> --artefact <path> --sbom <path> \\
              --id <id> --version <ver> [--component <kind>] --keystore <path>

OPTIONS:
  --registry   directory holding the durable artefact registry. Created if absent;
               every artefact already in it is re-verified when it is opened.
  --artefact   the proven binary emitted by `just gate` (target/verified/<pkg>).
  --sbom       the CycloneDX SBOM for that binary (<artefact>.sbom.cdx.json).
  --id         registry id for the artefact, e.g. the package name.
  --version    release version, e.g. 1.4.0.
  --component  component kind recorded in the manifest (default: service).
  --keystore   sealed keystore (mg-sign keygen) holding the registry's ADMISSION
               key, whose verifying key is the registry's trust anchor. This is
               not the release signing key: the registry pins ML-DSA-65
               (BIFROST_SIG_ALG) while the release root signs ML-DSA-87, so a
               keystore of the wrong algorithm is refused up front.
";

/// A usage or execution failure. Separated so `main` decides the exit code once.
enum Failure {
    Usage(String),
    Run(String),
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    match run(&argv) {
        Ok(msg) => println!("{msg}"),
        Err(Failure::Usage(e)) => {
            eprintln!("mg-register: {e}\n\n{USAGE}");
            exit(2);
        }
        Err(Failure::Run(e)) => {
            eprintln!("mg-register: {e}");
            exit(1);
        }
    }
}

/// Parse `--key value` pairs, rejecting anything unrecognised. A mistyped flag is
/// a usage error, never a silently ignored one — the difference between "signed
/// into the registry" and "quietly did nothing" must never be a typo.
fn parse(argv: &[String], allowed: &[&str]) -> Result<Vec<(String, String)>, Failure> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        let key = a
            .strip_prefix("--")
            .ok_or_else(|| Failure::Usage(format!("unexpected argument '{a}'")))?;
        if !allowed.contains(&key) {
            return Err(Failure::Usage(format!("unknown option '--{key}'")));
        }
        let val = argv
            .get(i + 1)
            .ok_or_else(|| Failure::Usage(format!("--{key} requires a value")))?;
        if val.starts_with("--") {
            return Err(Failure::Usage(format!("--{key} requires a value")));
        }
        out.push((key.to_string(), val.clone()));
        i += 2;
    }
    Ok(out)
}

fn get<'a>(args: &'a [(String, String)], key: &str) -> Option<&'a str> {
    args.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn require<'a>(args: &'a [(String, String)], key: &str) -> Result<&'a str, Failure> {
    get(args, key).ok_or_else(|| Failure::Usage(format!("--{key} is required")))
}

fn run(argv: &[String]) -> Result<String, Failure> {
    let allowed = [
        "registry",
        "artefact",
        "sbom",
        "id",
        "version",
        "component",
        "keystore",
    ];
    let args = parse(argv, &allowed)?;

    let registry_dir = require(&args, "registry")?;
    let artefact_path = require(&args, "artefact")?;
    let sbom_path = require(&args, "sbom")?;
    let id = require(&args, "id")?;
    let version = require(&args, "version")?;
    let keystore_path = require(&args, "keystore")?;
    let component = get(&args, "component").unwrap_or("service");

    let payload = std::fs::read(artefact_path)
        .map_err(|e| Failure::Run(format!("cannot read artefact {artefact_path}: {e}")))?;
    let sbom = std::fs::read_to_string(sbom_path)
        .map_err(|e| Failure::Run(format!("cannot read SBOM {sbom_path}: {e}")))?;

    // The admission key is unsealed from the keystore and never written anywhere.
    // Its verifying key doubles as the registry's trust anchor, so the registry
    // is by construction anchored to the same root that admitted the artefact.
    let keystore = attest::Keystore::read(Path::new(keystore_path))
        .map_err(|e| Failure::Run(format!("cannot open keystore {keystore_path}: {e}")))?;
    let signing_root = keystore
        .into_signing_root()
        .map_err(|e| Failure::Run(format!("cannot unseal the signing key: {e}")))?;

    // The registry admits one algorithm and one only (`BIFROST_SIG_ALG`), and it
    // is deliberately NOT the release root's (`kms::RELEASE_SIG_ALG`, ML-DSA-87):
    // two roots, two custody chains, so compromising either one neither forges a
    // release nor admits anything to a fleet. See docs/ops/PKI.md, "Signing roots
    // (two, deliberately)".
    //
    // Caught here, with the reason, rather than as an opaque "signature did not
    // verify" from the registry after the work of signing.
    let alg = signing_root.algorithm();
    if alg != bifrost_artefact::BIFROST_SIG_ALG {
        return Err(Failure::Run(format!(
            "keystore {keystore_path} signs with {}, but the Bifrost artefact registry \
             admits only {}. Use the registry's admission keystore \
             (`mg-sign keygen --alg {} --keystore <path>`), not the release signing keystore.",
            alg.as_str(),
            bifrost_artefact::BIFROST_SIG_ALG.as_str(),
            bifrost_artefact::BIFROST_SIG_ALG.as_str(),
        )));
    }

    let trust_key = signing_root.verifying_key().clone();

    let artefact = build_and_sign(
        id,
        version,
        component,
        payload,
        sbom,
        signing_root.expose_signing_key(),
    );

    // Opening re-verifies everything already resident; ingest verifies this one
    // before it is persisted. Either failing aborts the release.
    let mut registry = DurableArtefactRegistry::open(registry_dir, trust_key)
        .map_err(|e| Failure::Run(format!("cannot open registry {registry_dir}: {e}")))?;
    let reference = registry
        .ingest(artefact)
        .map_err(|e| Failure::Run(format!("registry rejected {id} {version}: {e}")))?;

    Ok(format!(
        "mg-register: registered {} {} ({component}) in {registry_dir} — {} artefact(s) resident",
        reference.id,
        reference.version,
        registry.len(),
    ))
}
