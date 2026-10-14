//! `mg-register` — hand a gate-verified artefact to Bifrost.
//!
//! This is the last stage of the release contract. `just gate <pkg>` proves the
//! artefact reproducible, SBOMs it, scans it, ML-DSA-signs it and verifies that
//! exact binary; this then admits it to the Bifrost artefact registry, which is
//! what makes it eligible for ring rollout and for air-gap bundling.
//!
//! ## It admits only what the gate signed
//! "Gate-verified" is checked here, not assumed. Before anything is signed for
//! admission, the artefact's release bundle — the binary, its `.sbom.cdx.json`,
//! its `.att` and its `.sig` — must verify against the pinned **release** trust
//! anchor, through the same [`attest::verify_artefact`] `mg-verify` runs. A
//! binary that never passed the gate has no such bundle and is refused; holding
//! the admission key is not enough to put something in front of a fleet.
//!
//! The bundle travels with the binary. Its `.sig` and `.att` (and the
//! reproducibility report, when given) are admitted as `attestation` artefacts
//! beside it, so the registry — the single place a released artefact is
//! recorded — holds the release signature's key id and the provenance, not just
//! the digests. [AC-RF-SC-5d]
//!
//! ## Two keys, both pinned
//! Admission is signed by the registry's own key (ML-DSA-65), a different root
//! from the release key (ML-DSA-87): compromising either one neither forges a
//! release nor admits anything to a fleet. The admission keystore must be the
//! one whose public half is pinned in the admission anchor file; a registry is
//! never anchored to whatever keystore happened to be passed. Signing happens
//! inside the custodian — the key is not taken out of it.
//! [FR-B-1] [FR-B-4] [AC-B-1] [NFR-PQ-3]
//!
//! ```text
//! mg-register [register] --registry <dir> --artefact <path> --id <id> --version <ver>
//!                        [--component <kind>] [--repro <path>] --keystore <path>
//!                        --release-anchor <file> --admission-anchor <file>
//! mg-register list       --registry <dir> --admission-anchor <file> [--key-id <id>]
//! ```
//!
//! Every input is explicit. There is no search path and no default location: a
//! release that cannot say which registry it is publishing to, under which key,
//! must fail rather than guess.

use std::path::Path;
use std::process::ExitCode;

use attest::{accept, envelope::SigEnvelope, preimage, TrustAnchors, APPROVED_RELEASE_ALGS};
use bifrost_artefact::{
    build_with_signer, generate_sbom, DurableArtefactRegistry, SbomComponent, SignedArtefact,
    BIFROST_SIG_ALG,
};
use kms::SigningRoot;
use pqc_agility::hash::sha256;
use pqc_agility::VerifyingKey;

const USAGE: &str = "\
mg-register — admit a gate-verified artefact to the Bifrost artefact registry

USAGE:
  mg-register [register] --registry <dir> --artefact <path> --id <id> --version <ver> \\
                         [--component <kind>] [--repro <path>] --keystore <path> \\
                         --release-anchor <file> --admission-anchor <file>
  mg-register list       --registry <dir> --admission-anchor <file> [--key-id <id>]

REGISTER:
  --registry          directory holding the durable artefact registry. Created if
                      absent; every artefact already in it is re-verified on open.
  --artefact          the proven binary emitted by `just gate`. Its release bundle
                      must sit beside it: <artefact>.sbom.cdx.json, .att and .sig.
  --id                registry id for the artefact, e.g. the binary name.
  --version           release version, e.g. 1.4.0.
  --component         component kind recorded in the manifest (default: service).
  --repro             the gate's reproducibility report for this artefact; admitted
                      beside it as <id>.repro.json.
  --keystore          sealed keystore holding the registry's ADMISSION key
                      (ML-DSA-65). Not the release signing key.
  --release-anchor    the pinned release trust anchor(s) (deploy/trust/release-trust.pub).
                      The artefact's bundle must verify against it, or nothing is admitted.
  --admission-anchor  the pinned admission anchor (deploy/trust/bifrost-trust.pub).
                      The keystore must be the key pinned there.

LIST:
  Prints one line per registered artefact — `<id> <version> <component> <sha256>` —
  sorted, after re-verifying every one of them. --key-id picks the anchor when the
  file pins more than one.
";

/// The component kind the bundle's own files are admitted under.
const ATTESTATION: &str = "attestation";

/// A usage or execution failure. Separated so `main` decides the exit code once.
#[derive(Debug)]
enum Failure {
    Usage(String),
    Run(String),
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // Help is its own invocation. It exits 0, so it must never be reachable from
    // a command line that also asks for a registration: a release script that
    // checks only the exit code would read that as "registered".
    if matches!(argv.as_slice(), [only] if is_help(only)) {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let result = match argv.first().map(String::as_str) {
        Some("list") => list(&argv[1..]),
        Some("register") => register(&argv[1..]),
        _ => register(&argv),
    };
    match result {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(Failure::Usage(e)) => {
            eprintln!("mg-register: {e}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Run(e)) => {
            eprintln!("mg-register: REFUSED — {e}");
            ExitCode::FAILURE
        }
    }
}

fn is_help(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
}

/// Parse `--key value` pairs, rejecting anything unrecognised. A mistyped flag is
/// a usage error, never a silently ignored one — the difference between "signed
/// into the registry" and "quietly did nothing" must never be a typo.
fn parse(argv: &[String], allowed: &[&str]) -> Result<Vec<(String, String)>, Failure> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        if is_help(a) {
            return Err(Failure::Usage(format!(
                "{a} must be the only argument — it does not register anything"
            )));
        }
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
        if out.iter().any(|(k, _): &(String, String)| k == key) {
            return Err(Failure::Usage(format!("--{key} given more than once")));
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

fn read_anchors(path: &str, what: &str) -> Result<TrustAnchors, Failure> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Failure::Run(format!("cannot read the {what} anchor {path}: {e}")))?;
    let anchors = TrustAnchors::parse(&text).map_err(|e| Failure::Run(format!("{path}: {e}")))?;
    if anchors.is_empty() {
        return Err(Failure::Run(format!("{path} pins no {what} key")));
    }
    Ok(anchors)
}

fn same_key(a: &VerifyingKey, b: &VerifyingKey) -> bool {
    a.alg() == b.alg() && a.as_bytes() == b.as_bytes()
}

fn register(argv: &[String]) -> Result<String, Failure> {
    let allowed = [
        "registry",
        "artefact",
        "id",
        "version",
        "component",
        "repro",
        "keystore",
        "release-anchor",
        "admission-anchor",
    ];
    let args = parse(argv, &allowed)?;

    let registry_dir = require(&args, "registry")?;
    let artefact_path = require(&args, "artefact")?;
    let id = require(&args, "id")?;
    let version = require(&args, "version")?;
    let keystore_path = require(&args, "keystore")?;
    let release_anchor = require(&args, "release-anchor")?;
    let admission_anchor = require(&args, "admission-anchor")?;
    let component = get(&args, "component").unwrap_or("service");
    if component == ATTESTATION {
        return Err(Failure::Usage(format!(
            "--component {ATTESTATION} is reserved for the bundle's own files"
        )));
    }

    // --- 1. The gate's verdict, re-established here ---------------------------
    // The same verifier CI and every image's entrypoint run. An artefact with no
    // release bundle, or one that does not verify against the pinned release
    // key, stops here: nothing has been signed for admission yet.
    let release_anchors = read_anchors(release_anchor, "release")?;
    let artefact = Path::new(artefact_path);
    let verdict = attest::verify_artefact(artefact, &release_anchors).map_err(|e| {
        Failure::Run(format!(
            "{artefact_path} has not passed the release gate — {e}"
        ))
    })?;

    // Read what will be admitted, then verify THOSE bytes. `verify_artefact` read
    // the files once; reading them again to store would admit whatever is on disk
    // by then. So the signature is checked a second time over the bytes in hand.
    let read = |path: &Path| {
        std::fs::read(path)
            .map_err(|e| Failure::Run(format!("cannot read {}: {e}", path.display())))
    };
    let payload = read(artefact)?;
    let sbom_bytes = read(&attest::sbom::sbom_path(artefact))?;
    let att = read(&attest::att_path(artefact))?;
    let sig = read(&attest::sig_path(artefact))?;

    let envelope =
        SigEnvelope::parse(&sig).map_err(|e| Failure::Run(format!("{artefact_path}.sig: {e}")))?;
    let release_key = release_anchors.get(&envelope.key_id).ok_or_else(|| {
        Failure::Run(format!(
            "release key {:?} is not pinned in {release_anchor}",
            envelope.key_id
        ))
    })?;
    let payload_sha256 = sha256(&payload);
    let sbom_sha256 = sha256(&sbom_bytes);
    accept::accept(
        &accept::RELEASE_ARTEFACT,
        &preimage::body(&payload_sha256, &sbom_sha256, &sha256(&att)),
        release_key,
        &envelope.signature,
        &APPROVED_RELEASE_ALGS,
    )
    .map_err(|e| {
        Failure::Run(format!(
            "{artefact_path} changed while it was being registered — {e}"
        ))
    })?;
    let sbom = String::from_utf8(sbom_bytes)
        .map_err(|_| Failure::Run(format!("{artefact_path}: the SBOM is not UTF-8")))?;

    let repro = match get(&args, "repro") {
        Some(p) => Some(read(Path::new(p))?),
        None => None,
    };

    // --- 2. The admission key, which must be the pinned one --------------------
    let keystore = attest::Keystore::read(Path::new(keystore_path))
        .map_err(|e| Failure::Run(format!("cannot open keystore {keystore_path}: {e}")))?;
    let signing_root = keystore
        .into_signing_root()
        .map_err(|e| Failure::Run(format!("cannot unseal the admission key: {e}")))?;

    // The registry admits one algorithm and one only (`BIFROST_SIG_ALG`), and it
    // is deliberately NOT the release root's (`kms::RELEASE_SIG_ALG`, ML-DSA-87):
    // two roots, two custody chains. See docs/ops/PKI.md, "Signing roots (two,
    // deliberately)". Caught here, with the reason, rather than as an opaque
    // "signature did not verify" from the registry after the work of signing.
    let alg = signing_root.algorithm();
    if alg != BIFROST_SIG_ALG {
        return Err(Failure::Run(format!(
            "keystore {keystore_path} signs with {}, but the Bifrost artefact registry \
             admits only {}. Use the registry's admission keystore \
             (`mg-sign keygen --alg {} --keystore <path>`), not the release signing keystore.",
            alg.as_str(),
            BIFROST_SIG_ALG.as_str(),
            BIFROST_SIG_ALG.as_str(),
        )));
    }

    // A registry is anchored to the PINNED admission key. Anchoring it to
    // whichever keystore was passed would let any ML-DSA-65 key start one.
    let admission_anchors = read_anchors(admission_anchor, "admission")?;
    let trust_key = signing_root.verifying_key().clone();
    match admission_anchors.get(signing_root.key_id()) {
        Some(pinned) if same_key(pinned, &trust_key) => {}
        Some(_) => {
            return Err(Failure::Run(format!(
                "keystore {keystore_path} holds a different key from the one {admission_anchor} \
                 pins as {:?}",
                signing_root.key_id()
            )))
        }
        None => {
            return Err(Failure::Run(format!(
                "keystore {keystore_path} (key id {:?}) is not the admission key pinned in \
                 {admission_anchor}",
                signing_root.key_id()
            )))
        }
    }

    // --- 3. Sign for admission, inside the custodian ---------------------------
    let admit = |id: String, component: &str, payload: Vec<u8>, sbom: String| {
        build_with_signer(id, version, component, payload, sbom, |msg| {
            signing_root.sign(msg)
        })
        .map_err(|e| Failure::Run(format!("the admission key could not sign: {e}")))
    };
    // The bundle's own files describe the artefact they belong to.
    let describes = |name: &str| {
        generate_sbom(
            name,
            version,
            ATTESTATION,
            vec![SbomComponent::new(id, version)],
        )
    };

    let mut batch: Vec<SignedArtefact> = vec![admit(id.to_string(), component, payload, sbom)?];
    let mut attestations = vec![(format!("{id}.sig"), sig), (format!("{id}.att"), att)];
    if let Some(bytes) = repro {
        attestations.push((format!("{id}.repro.json"), bytes));
    }
    for (name, bytes) in attestations {
        let sbom = describes(&name);
        batch.push(admit(name, ATTESTATION, bytes, sbom)?);
    }

    // --- 4. Admit ---------------------------------------------------------------
    // Opening re-verifies everything already resident; ingest verifies each of
    // these before it is persisted. Either failing aborts the release.
    let mut registry = DurableArtefactRegistry::open(registry_dir, trust_key)
        .map_err(|e| Failure::Run(format!("cannot open registry {registry_dir}: {e}")))?;
    let admitted = batch.len() - 1;
    for artefact in batch {
        let name = artefact.manifest.id.clone();
        registry
            .ingest(artefact)
            .map_err(|e| Failure::Run(format!("registry rejected {name} {version}: {e}")))?;
    }

    Ok(format!(
        "registered id={id} version={version} component={component} digest={} sbom={} \
         release-key={} attestations={admitted} resident={}",
        attest::hex::hex(&payload_sha256),
        attest::hex::hex(&sbom_sha256),
        verdict.key_id,
        registry.len(),
    ))
}

fn list(argv: &[String]) -> Result<String, Failure> {
    let args = parse(argv, &["registry", "admission-anchor", "key-id"])?;
    let registry_dir = require(&args, "registry")?;
    let admission_anchor = require(&args, "admission-anchor")?;

    let anchors = read_anchors(admission_anchor, "admission")?;
    let trust_key = match get(&args, "key-id") {
        Some(id) => anchors
            .get(id)
            .ok_or_else(|| Failure::Run(format!("{admission_anchor} pins no key {id:?}")))?,
        None => {
            let mut keys = anchors.iter();
            match (keys.next(), keys.next()) {
                (Some((_, key)), None) => key,
                _ => {
                    return Err(Failure::Usage(format!(
                        "{admission_anchor} pins {} keys; name the registry's with --key-id",
                        anchors.len()
                    )))
                }
            }
        }
    }
    .clone();

    // A directory that does not exist holds nothing, and must not be created by
    // a command that only reads.
    if !Path::new(registry_dir).is_dir() {
        return Err(Failure::Run(format!(
            "{registry_dir} is not a registry directory"
        )));
    }
    let registry = DurableArtefactRegistry::open(registry_dir, trust_key)
        .map_err(|e| Failure::Run(format!("cannot open registry {registry_dir}: {e}")))?;

    let mut lines = Vec::new();
    for r in registry.refs() {
        let artefact = registry
            .get(&r.id, &r.version)
            .expect("a listed reference resolves");
        lines.push(format!(
            "{} {} {} {}",
            r.id,
            r.version,
            artefact.manifest.component,
            attest::hex::hex(&artefact.manifest.payload_sha256)
        ));
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_mistyped_or_repeated_flag_is_a_usage_error() {
        let allowed = ["registry", "artefact"];
        assert!(matches!(
            parse(&argv(&["--registy", "x"]), &allowed),
            Err(Failure::Usage(_))
        ));
        assert!(matches!(
            parse(&argv(&["--registry"]), &allowed),
            Err(Failure::Usage(_))
        ));
        assert!(matches!(
            parse(&argv(&["--registry", "--artefact", "a"]), &allowed),
            Err(Failure::Usage(_))
        ));
        assert!(matches!(
            parse(&argv(&["--registry", "a", "--registry", "b"]), &allowed),
            Err(Failure::Usage(_))
        ));
        assert!(matches!(
            parse(&argv(&["stray"]), &allowed),
            Err(Failure::Usage(_))
        ));
    }

    #[test]
    fn help_beside_a_registration_is_a_usage_error() {
        for bad in [
            vec!["--registry", "r", "--help"],
            vec!["--help", "--registry", "r"],
            vec!["--registry", "r", "-h"],
        ] {
            assert!(
                matches!(register(&argv(&bad)), Err(Failure::Usage(_))),
                "{bad:?} must not be treated as a request for help"
            );
        }
    }

    #[test]
    fn every_input_must_be_named() {
        let all = [
            ("registry", "r"),
            ("artefact", "a"),
            ("id", "i"),
            ("version", "1"),
            ("keystore", "k"),
            ("release-anchor", "ra"),
            ("admission-anchor", "aa"),
        ];
        for missing in all.iter().map(|(k, _)| *k) {
            let mut args = Vec::new();
            for (k, v) in all.iter().filter(|(k, _)| *k != missing) {
                args.push(format!("--{k}"));
                args.push(v.to_string());
            }
            match register(&args) {
                Err(Failure::Usage(m)) => assert!(m.contains(missing), "{m}"),
                other => panic!("without --{missing}: expected a usage error, got {other:?}"),
            }
        }
    }

    #[test]
    fn the_attestation_component_cannot_be_claimed() {
        let args = argv(&[
            "--registry",
            "r",
            "--artefact",
            "a",
            "--id",
            "i",
            "--version",
            "1",
            "--keystore",
            "k",
            "--release-anchor",
            "ra",
            "--admission-anchor",
            "aa",
            "--component",
            "attestation",
        ]);
        assert!(matches!(register(&args), Err(Failure::Usage(_))));
    }
}
