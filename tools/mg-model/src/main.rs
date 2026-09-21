//! `mg-model` — sign a model's weights into the manifest the Mimir plane loads, and check
//! a model directory the way the plane does. Offline. [AC-MI-7] [FR-MS-4] [NFR-PQ-3]
//!
//! ```text
//! mg-model keygen --out DIR [--key-id ID]
//! mg-model sign   --weights FILE --id ID --family FAMILY --version V --quantisation Q
//!                 --licence SPDX --keystore FILE [--osi-approved] [--redistributable]
//!                 [--min-gpus N] [--min-vram-gb N] [--multinode] [--sha256 HEX] [--out FILE]
//! mg-model verify --dir DIR --trust FILE
//! mg-model pack   --dir DIR --model-trust FILE --keystore FILE --out FILE
//! mg-model unpack --bundle FILE --bundle-trust FILE --model-trust FILE --out DIR
//! ```
//!
//! `keygen` writes `DIR/model-signing.keystore`, the ML-DSA-65 signing key sealed at
//! rest, and `DIR/model-signing.pub`, the trust key `mimir-server --model-trust` reads.
//! It never overwrites either. Keep the keystore in staging; only the public key goes
//! into the enclosure.
//!
//! `sign` streams the weights through SHA-256 and recognises their format from their
//! first bytes: GGUF or safetensors, and nothing else. It signs the digest, the file's
//! name, format, quantisation and size, the family, the licence and the hardware the
//! model needs, and writes `model.manifest` beside the weights (or `--out`), never over
//! an existing file. With `--sha256`, the digest the upstream published must match what
//! was read, or nothing is signed.
//!
//! `verify` runs the plane's load gate on a model directory: the signature against the
//! trust key, the family, the hardware against this node, and the weights' size, format
//! and digest. Exit 1 on any refusal, 2 on bad usage.
//!
//! `pack` carries a model across the air gap as one signed bundle. [AC-MS-3] [FR-B-5]
//! It runs the load gate on `--dir` first, so only a model that verifies is packed.
//! It writes `model.manifest` and the weights as `model-bundle` artefacts, a chunk at a
//! time (bundle format version 2), so the weights are never held in memory. The bundle is
//! signed with the Bifrost admission keystore (`--keystore`, ML-DSA-65), not the model
//! key: a bundle proves how the files travelled, and the model manifest proves what they
//! are.
//!
//! `unpack` imports that bundle offline into `--out`, which must not exist. It checks
//! the bundle and every artefact against the admission anchors in `--bundle-trust`, a
//! chunk at a time. Then it runs the load gate on the directory it wrote. If the model
//! does not verify, the directory is removed, so nothing unverified is left to serve.

use std::path::{Path, PathBuf};
use std::process::exit;

use attest::Keystore;
use bifrost_artefact::{generate_sbom, SbomComponent, BIFROST_SIG_ALG};
use bifrost_bundle::{
    import_streaming, pack_streaming, BundleError, FileArtefact, DEFAULT_CHUNK_SIZE,
};
use model_server::{
    digest_weights, parse_trust_key, probe, render_manifest_file, sign_manifest, HwProfile,
    Licence, LicencePolicy, ModelLoader, ModelManifest, VerifiedModel, WeightsFile, MANIFEST_FILE,
    VELDRIS_MODEL_SIG_ALG,
};
use pqc_agility::{OsRng, SecretBytes, SigningKey, VerifyingKey};
use service_runtime::Args;

const USAGE: &str = "usage:
  mg-model keygen --out DIR [--key-id ID]
  mg-model sign   --weights FILE --id ID --family FAMILY --version V --quantisation Q
                  --licence SPDX --keystore FILE [--osi-approved] [--redistributable]
                  [--min-gpus N] [--min-vram-gb N] [--multinode] [--sha256 HEX] [--out FILE]
  mg-model verify --dir DIR --trust FILE
  mg-model pack   --dir DIR --model-trust FILE --keystore FILE --out FILE
  mg-model unpack --bundle FILE --bundle-trust FILE --model-trust FILE --out DIR";

/// The component kind a model's files travel as.
const MODEL_BUNDLE: &str = "model-bundle";

/// The signing keystore's file name.
const KEYSTORE_FILE: &str = "model-signing.keystore";
/// The exported trust key's file name.
const TRUST_FILE: &str = "model-signing.pub";

enum Failure {
    Usage(String),
    Run(String),
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = argv.split_first() else {
        eprintln!("{USAGE}");
        exit(2);
    };
    let result = match command.as_str() {
        "keygen" => parse(rest, &["out", "key-id"], &[]).and_then(|a| keygen(&a)),
        "sign" => parse(
            rest,
            &[
                "weights",
                "id",
                "family",
                "version",
                "quantisation",
                "licence",
                "keystore",
                "min-gpus",
                "min-vram-gb",
                "sha256",
                "out",
            ],
            &["osi-approved", "redistributable", "multinode"],
        )
        .and_then(|a| sign(&a)),
        "verify" => parse(rest, &["dir", "trust"], &[]).and_then(|a| verify(&a)),
        "pack" => {
            parse(rest, &["dir", "model-trust", "keystore", "out"], &[]).and_then(|a| pack(&a))
        }
        "unpack" => parse(rest, &["bundle", "bundle-trust", "model-trust", "out"], &[])
            .and_then(|a| unpack(&a)),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            return;
        }
        other => Err(Failure::Usage(format!("unknown command {other:?}"))),
    };
    match result {
        Ok(message) => println!("{message}"),
        Err(Failure::Usage(e)) => {
            eprintln!("mg-model: {e}\n{USAGE}");
            exit(2);
        }
        Err(Failure::Run(e)) => {
            eprintln!("mg-model: {e}");
            exit(1);
        }
    }
}

fn parse(argv: &[String], valued: &[&str], flags: &[&str]) -> Result<Args, Failure> {
    Args::parse(argv, valued, flags).map_err(Failure::Usage)
}

fn require<'a>(args: &'a Args, key: &str) -> Result<&'a str, Failure> {
    args.require(key).map_err(Failure::Usage)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn keygen(args: &Args) -> Result<String, Failure> {
    let out = Path::new(require(args, "out")?);
    let key_id = args.get("key-id").unwrap_or("model-signing");
    if key_id.is_empty()
        || !key_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(Failure::Usage(
            "--key-id may hold only letters, digits, '-', '_' and '.'".into(),
        ));
    }
    let keystore_path = out.join(KEYSTORE_FILE);
    let trust_path = out.join(TRUST_FILE);
    for path in [&keystore_path, &trust_path] {
        if path.exists() {
            return Err(Failure::Run(format!(
                "{} exists; keygen never overwrites a key",
                path.display()
            )));
        }
    }
    std::fs::create_dir_all(out)
        .map_err(|e| Failure::Run(format!("cannot create {}: {e}", out.display())))?;
    let key = Keystore::generate(key_id, VELDRIS_MODEL_SIG_ALG, &mut OsRng)
        .map_err(|e| Failure::Run(format!("cannot generate the signing key: {e}")))?;
    key.write(&keystore_path)
        .map_err(|e| Failure::Run(format!("cannot write the keystore: {e}")))?;
    std::fs::write(&trust_path, key.export_verifying_key())
        .map_err(|e| Failure::Run(format!("cannot write {}: {e}", trust_path.display())))?;
    Ok(format!(
        "wrote {} (keep it in staging) and {} (for mimir-server --model-trust)",
        keystore_path.display(),
        trust_path.display()
    ))
}

fn sign(args: &Args) -> Result<String, Failure> {
    let weights = Path::new(require(args, "weights")?);
    let name = weights
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Failure::Usage("--weights must name a file".into()))?;
    let count = |key: &str| -> Result<u32, Failure> {
        match args.get(key) {
            None => Ok(0),
            Some(v) => v
                .parse()
                .map_err(|_| Failure::Usage(format!("--{key} must be a number"))),
        }
    };
    let hw = HwProfile {
        min_gpus: count("min-gpus")?,
        min_vram_gb: count("min-vram-gb")?,
        multinode: args.flag("multinode"),
    };
    let (id, family, version, quantisation, licence) = (
        require(args, "id")?,
        require(args, "family")?,
        require(args, "version")?,
        require(args, "quantisation")?,
        require(args, "licence")?,
    );
    let out = args
        .get("out")
        .map(PathBuf::from)
        .unwrap_or_else(|| weights.with_file_name(MANIFEST_FILE));
    if out.exists() {
        return Err(Failure::Run(format!(
            "{} exists; sign never overwrites a manifest",
            out.display()
        )));
    }

    let mut file = std::fs::File::open(weights)
        .map_err(|e| Failure::Run(format!("cannot open {}: {e}", weights.display())))?;
    let found = digest_weights(&mut file)
        .map_err(|e| Failure::Run(format!("cannot read {}: {e}", weights.display())))?;
    let format = found.format.ok_or_else(|| {
        Failure::Run(format!(
            "{} is neither GGUF nor safetensors; nothing else is signed",
            weights.display()
        ))
    })?;
    let digest = hex(&found.sha256);
    if let Some(published) = args.get("sha256") {
        if !published.trim().eq_ignore_ascii_case(&digest) {
            return Err(Failure::Run(format!(
                "the weights' SHA-256 is {digest}, not the published {published}; nothing signed"
            )));
        }
    }

    let keystore_path = require(args, "keystore")?;
    let keystore = Keystore::read(Path::new(keystore_path))
        .map_err(|e| Failure::Run(format!("cannot open the keystore: {e}")))?;
    if keystore.alg != VELDRIS_MODEL_SIG_ALG {
        return Err(Failure::Run(format!(
            "{keystore_path} holds a {} key; model manifests are signed with {}",
            keystore.alg.as_str(),
            VELDRIS_MODEL_SIG_ALG.as_str()
        )));
    }
    let alg = keystore.alg;
    let root = keystore
        .into_signing_root()
        .map_err(|e| Failure::Run(format!("cannot unseal the signing key: {e}")))?;
    let signing_key = SigningKey::new(
        alg,
        SecretBytes::new(root.expose_signing_key().expose().to_vec()),
    );

    let manifest = ModelManifest {
        id: id.to_string(),
        family: family.to_string(),
        version: version.to_string(),
        sha256: found.sha256,
        hw,
        licence: Licence {
            name: licence.to_string(),
            osi_approved: args.flag("osi-approved"),
            redistribution_ok: args.flag("redistributable"),
        },
        weights: Some(WeightsFile {
            name: name.to_string(),
            format,
            quantisation: quantisation.to_string(),
            size: found.size,
        }),
    };
    let text =
        render_manifest_file(&sign_manifest(manifest, &signing_key)).map_err(Failure::Usage)?;
    std::fs::write(&out, text)
        .map_err(|e| Failure::Run(format!("cannot write {}: {e}", out.display())))?;
    Ok(format!(
        "signed {id} ({family}): {name}, {} bytes, {}, sha256 {digest} -> {}",
        found.size,
        format.as_str(),
        out.display()
    ))
}

/// Run the plane's load gate on a model directory, trusting the key in `trust_path`.
fn load_gate(dir: &Path, trust_path: &str) -> Result<VerifiedModel, Failure> {
    let trust = std::fs::read_to_string(trust_path)
        .map_err(|e| format!("cannot read {trust_path}: {e}"))
        .and_then(|text| parse_trust_key(&text).map_err(|e| format!("{trust_path}: {e}")))
        .map_err(Failure::Run)?;
    ModelLoader::new(trust, LicencePolicy::default())
        .verify_dir(dir, &probe())
        .map_err(Failure::Run)
}

/// The Bifrost admission anchors (ML-DSA-65) in a trust-anchor file.
fn bundle_anchors(path: &str) -> Result<Vec<VerifyingKey>, Failure> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Failure::Run(format!("cannot read {path}: {e}")))?;
    let mut keys = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(Failure::Run(format!(
                "{path}:{}: expected `<alg> <key-id> <hex>`",
                n + 1
            )));
        }
        if parts[0] == BIFROST_SIG_ALG.as_str() {
            let bytes = attest::hex::unhex(parts[2])
                .ok_or_else(|| Failure::Run(format!("{path}:{}: the key is not hex", n + 1)))?;
            keys.push(VerifyingKey::new(BIFROST_SIG_ALG, bytes));
        }
    }
    if keys.is_empty() {
        return Err(Failure::Run(format!(
            "{path} holds no {} anchor to verify a bundle with",
            BIFROST_SIG_ALG.as_str()
        )));
    }
    Ok(keys)
}

fn pack(args: &Args) -> Result<String, Failure> {
    let dir = Path::new(require(args, "dir")?);
    // Only a model that passes the load gate is packed.
    let model = load_gate(dir, require(args, "model-trust")?)?;
    let out = Path::new(require(args, "out")?);
    if out.exists() {
        return Err(Failure::Run(format!(
            "{} exists; pack never overwrites a bundle",
            out.display()
        )));
    }
    let keystore_path = require(args, "keystore")?;
    let keystore = Keystore::read(Path::new(keystore_path))
        .map_err(|e| Failure::Run(format!("cannot open the keystore: {e}")))?;
    if keystore.alg != BIFROST_SIG_ALG {
        return Err(Failure::Run(format!(
            "{keystore_path} holds a {} key; bundles are signed with the Bifrost admission \
             key ({})",
            keystore.alg.as_str(),
            BIFROST_SIG_ALG.as_str()
        )));
    }
    let alg = keystore.alg;
    let root = keystore
        .into_signing_root()
        .map_err(|e| Failure::Run(format!("cannot unseal the signing key: {e}")))?;
    let signing_key = SigningKey::new(
        alg,
        SecretBytes::new(root.expose_signing_key().expose().to_vec()),
    );

    let manifest = &model.artifact.manifest;
    let weights = manifest
        .weights
        .as_ref()
        .expect("a verified manifest names its weights file");
    let artefact = |name: &str, path: PathBuf| FileArtefact {
        id: name.to_string(),
        version: manifest.version.clone(),
        component: MODEL_BUNDLE.to_string(),
        sbom: generate_sbom(
            name,
            &manifest.version,
            MODEL_BUNDLE,
            vec![SbomComponent::new(&manifest.id, &manifest.version)],
        ),
        path,
    };
    let artefacts = [
        artefact(MANIFEST_FILE, dir.join(MANIFEST_FILE)),
        artefact(&weights.name, model.weights_path.clone()),
    ];
    pack_streaming(
        out,
        manifest.id.as_str(),
        format!("{}@{}", manifest.id, manifest.version),
        &artefacts,
        &signing_key,
        DEFAULT_CHUNK_SIZE,
    )
    .map_err(|e| Failure::Run(format!("cannot pack {}: {e}", out.display())))?;
    Ok(format!(
        "packed {} ({}): {} and {} ({} bytes) -> {}",
        manifest.id,
        manifest.family,
        MANIFEST_FILE,
        weights.name,
        weights.size,
        out.display()
    ))
}

fn unpack(args: &Args) -> Result<String, Failure> {
    let bundle = require(args, "bundle")?;
    let out = Path::new(require(args, "out")?);
    if out.exists() {
        return Err(Failure::Run(format!(
            "{} exists; unpack writes a new directory",
            out.display()
        )));
    }
    let keys = bundle_anchors(require(args, "bundle-trust")?)?;
    let model_trust = require(args, "model-trust")?;

    // A key that did not sign the bundle is refused at its signature, before any
    // payload is read, so the next anchor is tried; any other refusal is final.
    let mut refused = None;
    let mut imported = None;
    for vk in &keys {
        match import_streaming(bundle, vk, Some(out)) {
            Ok(refs) => {
                imported = Some(refs);
                break;
            }
            Err(BundleError::BadBundleSignature) => refused = Some(BundleError::BadBundleSignature),
            Err(e) => {
                refused = Some(e);
                break;
            }
        }
    }
    let refs = imported.ok_or_else(|| {
        Failure::Run(format!(
            "bundle REJECTED: {}",
            refused.map_or_else(|| "no trust anchor".to_string(), |e| e.to_string())
        ))
    })?;

    // The bundle proves how the files travelled; the load gate proves what they are.
    match load_gate(out, model_trust) {
        Ok(model) => Ok(format!(
            "unpacked and verified {} ({}): {} artefact(s) from {bundle} -> {}",
            model.loaded.id,
            model.loaded.family,
            refs.len(),
            out.display()
        )),
        Err(Failure::Run(e) | Failure::Usage(e)) => {
            let _ = std::fs::remove_dir_all(out);
            Err(Failure::Run(format!(
                "the unpacked model does not verify, so {} was removed: {e}",
                out.display()
            )))
        }
    }
}

fn verify(args: &Args) -> Result<String, Failure> {
    let dir = Path::new(require(args, "dir")?);
    let model = load_gate(dir, require(args, "trust")?)?;
    let manifest = &model.artifact.manifest;
    let weights = manifest
        .weights
        .as_ref()
        .expect("a verified manifest names its weights file");
    Ok(format!(
        "verified {} ({}): {}, {} bytes, {}, sha256 {}",
        model.loaded.id,
        model.loaded.family,
        weights.name,
        weights.size,
        weights.format.as_str(),
        hex(&manifest.sha256)
    ))
}
