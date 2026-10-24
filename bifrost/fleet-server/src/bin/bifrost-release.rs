//! `bifrost-release` — the release tooling's client for the artefact registry: sign a
//! build's Bifrost manifest with the release keystore, upload it to `bifrost-server`,
//! and read the registry back. `tools/bifrost/release.sh` drives it. [FR-B-1] [FR-B-4]
//! [NFR-PQ-3]
//!
//! ```text
//! bifrost-release sign      --keystore KS --id ID --version V [--component C]
//!                           --payload FILE --sbom FILE --out SIGNED.json
//! bifrost-release upload    --identity-dir DIR --addr HOST:PORT [--service NAME]
//!                           --signed SIGNED.json --payload FILE --sbom FILE
//! bifrost-release list      --identity-dir DIR --addr HOST:PORT [--service NAME]
//!                           [--version V] [--json]
//! bifrost-release preflight --identity-dir DIR --addr HOST:PORT [--service NAME]
//!                           --keystore KS
//! ```
//!
//! Signing and uploading are separate steps. `sign` needs only the keystore, no
//! server: it signs the manifest `{id, version, component, payloadSha256, sbomSha256}`
//! in the `BIFROST_ARTEFACT` domain inside the custodian (as `mg-register` does; the
//! key never leaves the sealed keystore) and writes the manifest, the signer's key id
//! and the signature to `SIGNED.json`. `upload` sends that signed manifest with the
//! payload and SBOM files as they are now, over PQ-mTLS, in chunks
//! (`RegisterArtefact`, `PutArtefactChunk`, `CommitArtefact`). It does not check them
//! itself: the registry's gate is the check, so a file changed after signing is
//! refused there and recorded as rejected.
//!
//! `--service` names the client identity in `--identity-dir` (default
//! `release-manager`, as `tools/bifrost/run-local.sh setup` issues it). `preflight`
//! checks, before a release builds anything, that the server answers, that this
//! identity may read the registry, and that the keystore's key is one of the
//! registry's release signers. Exit codes: 0 done, 1 refused or failed, 2 usage.

use std::path::Path;

use attest::{Keystore, KmsSigner, SigningBackend};
use bifrost_artefact::{build_with_signer, ArtefactManifest, SignedArtefact, BIFROST_SIG_ALG};
use bifrost_fleet_server::client::{self, CallError};
use pqc_agility::{SigId, Signature};
use service_runtime::client::Session;
use service_runtime::json::{parse, Json};
use service_runtime::{Args, ServiceIdentity};

const USAGE: &str = "usage:
  bifrost-release sign      --keystore KS --id ID --version V [--component C] \\
                            --payload FILE --sbom FILE --out SIGNED.json
  bifrost-release upload    --identity-dir DIR --addr HOST:PORT [--service NAME] \\
                            --signed SIGNED.json --payload FILE --sbom FILE
  bifrost-release list      --identity-dir DIR --addr HOST:PORT [--service NAME] \\
                            [--version V] [--json]
  bifrost-release preflight --identity-dir DIR --addr HOST:PORT [--service NAME] \\
                            --keystore KS";

/// The mesh purpose the registry routes are served under.
const PURPOSE: &str = "fleet-management";

enum Failure {
    Usage(String),
    Run(String),
}

impl From<CallError> for Failure {
    fn from(e: CallError) -> Self {
        Failure::Run(e.to_string())
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let result = match argv.split_first() {
        Some((cmd, rest)) => match cmd.as_str() {
            "sign" => sign(rest),
            "upload" => upload(rest),
            "list" => list(rest),
            "preflight" => preflight(rest),
            "-h" | "--help" | "help" if rest.is_empty() => {
                println!("{USAGE}");
                return;
            }
            other => Err(Failure::Usage(format!("unknown command {other:?}"))),
        },
        None => Err(Failure::Usage("a command is required".into())),
    };
    match result {
        Ok(out) => println!("{out}"),
        Err(Failure::Usage(e)) => {
            eprintln!("bifrost-release: {e}\n{USAGE}");
            std::process::exit(2);
        }
        Err(Failure::Run(e)) => {
            eprintln!("bifrost-release: {e}");
            std::process::exit(1);
        }
    }
}

fn args(argv: &[String], valued: &[&str], flags: &[&str]) -> Result<Args, Failure> {
    Args::parse(argv, valued, flags).map_err(Failure::Usage)
}

fn require<'a>(args: &'a Args, key: &str) -> Result<&'a str, Failure> {
    args.require(key).map_err(Failure::Usage)
}

fn read(path: &str) -> Result<Vec<u8>, Failure> {
    std::fs::read(path).map_err(|e| Failure::Run(format!("cannot read {path}: {e}")))
}

fn read_text(path: &str) -> Result<String, Failure> {
    String::from_utf8(read(path)?).map_err(|_| Failure::Run(format!("{path} is not UTF-8")))
}

/// The release keystore's custodian. Only ML-DSA-65, the one algorithm the registry
/// admits, is accepted, with the reason, before anything is signed.
fn signer(path: &str) -> Result<impl SigningBackend, Failure> {
    let keystore = Keystore::read(Path::new(path))
        .map_err(|e| Failure::Run(format!("cannot open keystore {path}: {e}")))?;
    let root = keystore
        .into_signing_root()
        .map_err(|e| Failure::Run(format!("cannot unseal the release key in {path}: {e}")))?;
    let signer = KmsSigner::new(root);
    let alg = signer.verifying_key().alg();
    if alg != BIFROST_SIG_ALG {
        return Err(Failure::Run(format!(
            "keystore {path} signs with {}, but the registry admits only {}; make the \
             release keystore with `mg-sign keygen --alg {}`",
            alg.as_str(),
            BIFROST_SIG_ALG.as_str(),
            BIFROST_SIG_ALG.as_str(),
        )));
    }
    Ok(signer)
}

fn sign(argv: &[String]) -> Result<String, Failure> {
    let a = args(
        argv,
        &[
            "keystore",
            "id",
            "version",
            "component",
            "payload",
            "sbom",
            "out",
        ],
        &[],
    )?;
    let signer = signer(require(&a, "keystore")?)?;
    let id = require(&a, "id")?;
    let version = require(&a, "version")?;
    let component = a.get("component").unwrap_or("service");
    let payload = read(require(&a, "payload")?)?;
    let sbom = read_text(require(&a, "sbom")?)?;
    let out = require(&a, "out")?;

    let signed = build_with_signer(id, version, component, payload, sbom, |msg| {
        signer.sign(msg)
    })
    .map_err(|e| Failure::Run(format!("the release key could not sign: {e}")))?;
    let m = &signed.manifest;
    let doc = Json::obj(vec![
        ("manifest", manifest_json(m)),
        ("signer", Json::s(signer.key_id())),
        ("alg", Json::s(signed.signature.alg().as_str())),
        (
            "signature",
            Json::s(&attest::hex::hex(signed.signature.as_bytes())),
        ),
    ]);
    std::fs::write(out, doc.encode() + "\n")
        .map_err(|e| Failure::Run(format!("cannot write {out}: {e}")))?;
    Ok(format!(
        "signed {id} {version} ({component}) payload={} sbom={} signer={}",
        attest::hex::hex(&m.payload_sha256),
        attest::hex::hex(&m.sbom_sha256),
        signer.key_id()
    ))
}

fn manifest_json(m: &ArtefactManifest) -> Json {
    Json::obj(vec![
        ("id", Json::s(&m.id)),
        ("version", Json::s(&m.version)),
        ("component", Json::s(&m.component)),
        (
            "payloadSha256",
            Json::s(&attest::hex::hex(&m.payload_sha256)),
        ),
        ("sbomSha256", Json::s(&attest::hex::hex(&m.sbom_sha256))),
    ])
}

/// Read a `sign` output back: the manifest, the signer's key id and the signature.
fn read_signed(path: &str) -> Result<(ArtefactManifest, String, Signature), Failure> {
    let doc = parse(&read_text(path)?).map_err(|e| Failure::Run(format!("{path}: {e}")))?;
    let field = |j: &Json, k: &str| {
        j.str_field(k)
            .map(str::to_string)
            .ok_or_else(|| Failure::Run(format!("{path}: no {k}")))
    };
    let digest = |j: &Json, k: &str| {
        attest::hex::unhex(&field(j, k)?)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            .ok_or_else(|| Failure::Run(format!("{path}: {k} is not a SHA-256 digest")))
    };
    let m = doc
        .get("manifest")
        .ok_or_else(|| Failure::Run(format!("{path}: no manifest")))?;
    let manifest = ArtefactManifest {
        id: field(m, "id")?,
        version: field(m, "version")?,
        component: field(m, "component")?,
        payload_sha256: digest(m, "payloadSha256")?,
        sbom_sha256: digest(m, "sbomSha256")?,
    };
    let alg = field(&doc, "alg")?;
    let alg = [SigId::MlDsa44, SigId::MlDsa65, SigId::MlDsa87]
        .into_iter()
        .find(|a| a.as_str() == alg)
        .ok_or_else(|| Failure::Run(format!("{path}: unknown signature algorithm {alg}")))?;
    let signature = attest::hex::unhex(&field(&doc, "signature")?)
        .ok_or_else(|| Failure::Run(format!("{path}: the signature is not hex")))?;
    Ok((
        manifest,
        field(&doc, "signer")?,
        Signature::new(alg, signature),
    ))
}

fn session(a: &Args) -> Result<Session, Failure> {
    let service = a.get("service").unwrap_or("release-manager");
    let dir = require(a, "identity-dir")?;
    let identity = ServiceIdentity::load(dir, service)
        .map_err(|e| Failure::Run(format!("cannot load the identity {service} in {dir}: {e}")))?;
    let addr = require(a, "addr")?;
    Session::open(&identity, addr, PURPOSE)
        .map_err(|e| Failure::Run(format!("cannot reach bifrost-server at {addr}: {e}")))
}

fn upload(argv: &[String]) -> Result<String, Failure> {
    let a = args(
        argv,
        &[
            "identity-dir",
            "addr",
            "service",
            "signed",
            "payload",
            "sbom",
        ],
        &[],
    )?;
    let (manifest, signer, signature) = read_signed(require(&a, "signed")?)?;
    let artefact = SignedArtefact {
        manifest,
        sbom: read_text(require(&a, "sbom")?)?,
        payload: read(require(&a, "payload")?)?,
        signature,
    };
    let (id, version) = (
        artefact.manifest.id.clone(),
        artefact.manifest.version.clone(),
    );
    let mut s = session(&a)?;
    let listed = client::register(&mut s, &artefact, &signer)
        .map_err(|e| Failure::Run(format!("{id} {version}: {e}")))?;
    Ok(format!("{id} {version}: {}", row(&listed)))
}

/// One listing item as a line: status, size, signer, payload digest, or the reason.
fn row(item: &Json) -> String {
    let f = |k: &str| item.str_field(k).unwrap_or("-");
    match f("status") {
        "rejected" => format!("rejected ({}: {})", f("reason"), f("detail")),
        status => {
            let size = item.get("size").and_then(Json::as_i64).unwrap_or(0);
            let mut line = format!(
                "{status} size={size} signer={} sha256={}",
                f("signer"),
                f("payloadSha256")
            );
            if let Some(detail) = item.str_field("detail") {
                line.push_str(&format!(" ({detail})"));
            }
            line
        }
    }
}

fn list(argv: &[String]) -> Result<String, Failure> {
    let a = args(
        argv,
        &["identity-dir", "addr", "service", "version"],
        &["json"],
    )?;
    let mut s = session(&a)?;
    let listing = client::list(&mut s)?;
    if a.flag("json") {
        return Ok(listing.encode());
    }
    let signers = match listing.get("signers") {
        Some(Json::Arr(items)) => items
            .iter()
            .filter_map(Json::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    };
    let mut lines = vec![format!("release signers: {signers}")];
    if let Some(Json::Arr(items)) = listing.get("artefacts") {
        for item in items {
            let version = item.str_field("version").unwrap_or("-");
            if a.get("version").is_some_and(|v| v != version) {
                continue;
            }
            lines.push(format!(
                "{} {version} {}",
                item.str_field("id").unwrap_or("-"),
                row(item)
            ));
        }
    }
    Ok(lines.join("\n"))
}

fn preflight(argv: &[String]) -> Result<String, Failure> {
    let a = args(argv, &["identity-dir", "addr", "service", "keystore"], &[])?;
    let keystore = require(&a, "keystore")?;
    let signer = signer(keystore)?;
    let mut s = session(&a)?;
    let listing = client::list(&mut s)?;
    let anchored = match listing.get("signers") {
        Some(Json::Arr(items)) => items.iter().any(|i| i.as_str() == Some(signer.key_id())),
        _ => false,
    };
    if !anchored {
        return Err(Failure::Run(format!(
            "the registry does not trust the release key {:?} (from {keystore}): install its \
             .pub into the server's trust/release-signers/ and restart bifrost-server",
            signer.key_id()
        )));
    }
    Ok(format!(
        "bifrost-server at {} trusts release key {}",
        require(&a, "addr")?,
        signer.key_id()
    ))
}
