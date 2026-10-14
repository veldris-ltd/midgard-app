//! mg-bundle — produce and consume signed offline release bundles. [FR-B-5] [RC-19]
//! [AC-B-3]
//!
//! Producer (connected side):
//!   mg-bundle pack --out FILE --id ID --label LABEL (--keystore KEYSTORE | --key KEYFILE)
//!                  SPEC...
//!     where SPEC = [<component>=]<id>:<version>:<path>
//!     <component> is service (the default), tool, client, model-bundle or attestation.
//!     An adjacent <path>.sbom.cdx.json is bundled as the artefact's SBOM if present.
//!
//! Consumer (air-gapped site, fully offline):
//!   mg-bundle import (--trust-anchor ANCHORS | --trust-key TRUSTFILE) [--out-dir DIR] FILE
//!     verifies the bundle signature, the manifest against the contents and every
//!     contained artefact, and prints the deployable refs (`id@version`). Exits
//!     non-zero (fail closed) on any verification failure. With --out-dir — only once
//!     everything has verified — writes each artefact's payload to DIR/<id> and its
//!     SBOM to DIR/<id>.sbom.cdx.json, refusing to overwrite a file.
//!
//! Keys. A bundle and every artefact in it are signed with the Bifrost admission key,
//! ML-DSA-65 (`BIFROST_SIG_ALG`), the only algorithm import accepts:
//!   --keystore      a sealed keystore (`mg-sign keygen --alg ML-DSA-65`); the signing key
//!                   is unsealed in memory only
//!   --trust-anchor  the `<alg> <key-id> <hex>` lines a keygen exports; each ML-DSA-65
//!                   anchor in the file is tried in turn
//!   --key, --trust-key  the older plaintext `alg=`/`keyid=`/`vk=`/`sk=` hex files, kept
//!                   for development and tests; a release signs from the keystore.
//!
//! Size. `pack` writes format version 2 and `import` reads it a chunk at a time
//! (`bifrost_bundle::stream`), so an artefact larger than memory — a model's weights —
//! can be packed and imported. `import` still reads version 1 bundles.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bifrost_artefact::{generate_sbom, SbomComponent, BIFROST_SIG_ALG};
use bifrost_bundle::{
    import_streaming, pack_streaming, BundleError, FileArtefact, DEFAULT_CHUNK_SIZE,
};
use pqc_agility::{SecretBytes, SigId, SigningKey, VerifyingKey};

/// The components an artefact in a bundle may be.
const COMPONENTS: [&str; 5] = ["service", "tool", "client", "model-bundle", "attestation"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("pack") => pack(&args[1..]),
        Some("import") => do_import(&args[1..]),
        _ => {
            eprintln!("usage: mg-bundle <pack|import> ...");
            ExitCode::from(2)
        }
    }
}

/// One artefact to pack.
#[derive(Debug, PartialEq, Eq)]
struct Spec {
    component: String,
    id: String,
    version: String,
    path: String,
}

/// Parse `[<component>=]<id>:<version>:<path>`. The path is everything after the second
/// colon, so a Windows path keeps its drive letter; an `=` inside the path is not taken
/// for a component.
fn parse_spec(spec: &str) -> Result<Spec, String> {
    let (component, rest) = match spec.split_once('=') {
        Some((component, rest)) if !component.contains(':') => (component, rest),
        _ => ("service", spec),
    };
    if !COMPONENTS.contains(&component) {
        return Err(format!(
            "unknown component {component:?} in {spec:?} (want one of {})",
            COMPONENTS.join(", ")
        ));
    }
    let parts: Vec<&str> = rest.splitn(3, ':').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return Err(format!(
            "bad SPEC {spec:?} (want [<component>=]<id>:<version>:<path>)"
        ));
    }
    check_id(parts[0])?;
    Ok(Spec {
        component: component.to_string(),
        id: parts[0].to_string(),
        version: parts[1].to_string(),
        path: parts[2].to_string(),
    })
}

/// An artefact id names the file it is written to on import: a plain file name.
fn check_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.starts_with('.') || id.contains(['/', '\\', ':']) {
        return Err(format!("artefact id {id:?} must be a plain file name"));
    }
    Ok(())
}

fn pack(args: &[String]) -> ExitCode {
    let mut out = None;
    let mut id = None;
    let mut label = None;
    let mut key = None;
    let mut keystore = None;
    let mut specs = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = it.next().cloned(),
            "--id" => id = it.next().cloned(),
            "--label" => label = it.next().cloned(),
            "--key" => key = it.next().cloned(),
            "--keystore" => keystore = it.next().cloned(),
            other => specs.push(other.to_string()),
        }
    }
    let (out, id, label) = match (out, id, label) {
        (Some(o), Some(i), Some(l)) if !specs.is_empty() => (o, i, l),
        _ => {
            return fail(
                "pack requires --out --id --label, --keystore or --key, and at least one SPEC",
            )
        }
    };
    let specs = match specs
        .iter()
        .map(|s| parse_spec(s))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(specs) => specs,
        Err(e) => return fail(&e),
    };
    let signing_key = match signing_key(keystore.as_deref(), key.as_deref()) {
        Ok(k) => k,
        Err(e) => return fail(&e),
    };

    let artefacts: Vec<FileArtefact> = specs
        .iter()
        .map(|spec| FileArtefact {
            id: spec.id.clone(),
            version: spec.version.clone(),
            component: spec.component.clone(),
            sbom: std::fs::read_to_string(format!("{}.sbom.cdx.json", spec.path)).unwrap_or_else(
                |_| {
                    generate_sbom(
                        &spec.id,
                        &spec.version,
                        &spec.component,
                        vec![SbomComponent::new(&spec.id, &spec.version)],
                    )
                },
            ),
            path: PathBuf::from(&spec.path),
        })
        .collect();

    // Import refuses an artefact whose SBOM is not a usable bill of materials, so
    // refuse it here too: a bundle that packs cleanly and can never be imported
    // is discovered on the wrong side of the air gap.
    for artefact in &artefacts {
        if let Err(e) = bifrost_artefact::check_sbom(&artefact.sbom) {
            return fail(&format!("{}: {e}", artefact.id));
        }
    }

    // A chunk at a time, so an artefact larger than memory — a model's weights — packs.
    match pack_streaming(
        &out,
        id,
        label,
        &artefacts,
        &signing_key,
        DEFAULT_CHUNK_SIZE,
    ) {
        Ok(manifest) => {
            eprintln!(
                "mg-bundle: packed {} artefact(s) -> {out}",
                manifest.entries.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("writing {out}: {e}")),
    }
}

fn do_import(args: &[String]) -> ExitCode {
    let mut anchors = None;
    let mut trust = None;
    let mut out_dir = None;
    let mut file = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--trust-anchor" => anchors = it.next().cloned(),
            "--trust-key" => trust = it.next().cloned(),
            "--out-dir" => out_dir = it.next().cloned(),
            other => file = Some(other.to_string()),
        }
    }
    let Some(file) = file else {
        return fail("import requires --trust-anchor or --trust-key, and a bundle FILE");
    };
    let keys = match trust_keys(anchors.as_deref(), trust.as_deref()) {
        Ok(k) => k,
        Err(e) => return fail(&e),
    };
    // Verify fully offline, a chunk at a time — nothing here touches the network. A key
    // that did not sign the bundle is refused at the bundle signature, before any payload
    // is read, so the next anchor is tried; any other refusal is final.
    let out = out_dir.as_deref().map(Path::new);
    let mut last_error = None;
    let mut verified = None;
    for vk in &keys {
        match import_streaming(&file, vk, out) {
            Ok(refs) => {
                verified = Some(refs);
                break;
            }
            Err(BundleError::BadBundleSignature) => {
                last_error = Some(BundleError::BadBundleSignature)
            }
            Err(e) => {
                last_error = Some(e);
                break;
            }
        }
    }
    let refs = match verified {
        Some(refs) => refs,
        None => {
            return fail(&format!(
                "bundle REJECTED: {}",
                last_error.map_or_else(|| "no trust anchor".to_string(), |e| e.to_string())
            ))
        }
    };

    eprintln!(
        "mg-bundle: OK — {} artefact(s) verified offline:",
        refs.len()
    );
    for r in refs {
        println!("{}@{}", r.id, r.version);
    }
    ExitCode::SUCCESS
}

/// The bundle signing key, from a sealed keystore or a plaintext key file. Only the
/// Bifrost admission algorithm is accepted: a bundle signed with anything else could
/// never be imported.
fn signing_key(keystore: Option<&str>, keyfile: Option<&str>) -> Result<SigningKey, String> {
    let key = match (keystore, keyfile) {
        (Some(path), None) => {
            let keystore = attest::Keystore::read(Path::new(path))
                .map_err(|e| format!("cannot open the keystore: {e}"))?;
            let alg = keystore.alg;
            let root = keystore
                .into_signing_root()
                .map_err(|e| format!("cannot unseal the signing key: {e}"))?;
            SigningKey::new(
                alg,
                SecretBytes::new(root.expose_signing_key().expose().to_vec()),
            )
        }
        (None, Some(path)) => read_signing_key(path)?,
        _ => return Err("pack needs exactly one of --keystore or --key".into()),
    };
    if key.alg() != BIFROST_SIG_ALG {
        return Err(format!(
            "the key is {}; bundles are signed with {} (the Bifrost admission key), the only \
             algorithm import accepts",
            key.alg().as_str(),
            BIFROST_SIG_ALG.as_str()
        ));
    }
    Ok(key)
}

/// The keys a bundle may verify under: every ML-DSA-65 anchor in a trust-anchor file,
/// or the one key in a plaintext trust file.
fn trust_keys(anchors: Option<&str>, trustfile: Option<&str>) -> Result<Vec<VerifyingKey>, String> {
    match (anchors, trustfile) {
        (Some(path), None) => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
            let mut keys = Vec::new();
            for (n, line) in text.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() != 3 {
                    return Err(format!("{path}:{}: expected `<alg> <key-id> <hex>`", n + 1));
                }
                if parts[0] == BIFROST_SIG_ALG.as_str() {
                    keys.push(VerifyingKey::new(BIFROST_SIG_ALG, hex_decode(parts[2])?));
                }
            }
            if keys.is_empty() {
                return Err(format!(
                    "{path} holds no {} anchor to verify a bundle with",
                    BIFROST_SIG_ALG.as_str()
                ));
            }
            Ok(keys)
        }
        (None, Some(path)) => Ok(vec![read_verifying_key(path)?]),
        _ => Err("import needs exactly one of --trust-anchor or --trust-key".into()),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("mg-bundle: {msg}");
    ExitCode::from(1)
}

// --- plaintext key material (alg=/keyid=/vk=/sk= hex) --------------------------

fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix(&format!("{key}=")))
        .map(str::trim)
}

fn alg_of(text: &str) -> Result<SigId, String> {
    match field(text, "alg") {
        Some("ML-DSA-44") => Ok(SigId::MlDsa44),
        Some("ML-DSA-65") => Ok(SigId::MlDsa65),
        Some("ML-DSA-87") => Ok(SigId::MlDsa87),
        other => Err(format!("unsupported/missing alg: {other:?}")),
    }
}

fn read_verifying_key(path: &str) -> Result<VerifyingKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let alg = alg_of(&text)?;
    let vk = hex_decode(field(&text, "vk").ok_or("missing vk")?)?;
    Ok(VerifyingKey::new(alg, vk))
}

fn read_signing_key(path: &str) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let alg = alg_of(&text)?;
    let sk = hex_decode(field(&text, "sk").ok_or("missing sk")?)?;
    Ok(SigningKey::new(alg, SecretBytes::new(sk)))
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err("odd-length hex".into());
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16).ok_or("bad hex")?;
        let lo = (b[i + 1] as char).to_digit(16).ok_or("bad hex")?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_name_a_component_or_default_to_service() {
        assert_eq!(
            parse_spec("http-gateway:0.1.0:target/verified/http-gateway").unwrap(),
            Spec {
                component: "service".into(),
                id: "http-gateway".into(),
                version: "0.1.0".into(),
                path: "target/verified/http-gateway".into(),
            }
        );
        let sig = parse_spec(r"attestation=mg-verify.sig:0.1.0:C:\out\mg-verify.sig").unwrap();
        assert_eq!(sig.component, "attestation");
        assert_eq!(sig.id, "mg-verify.sig");
        assert_eq!(sig.path, r"C:\out\mg-verify.sig");
        // An `=` in the path is not a component.
        assert_eq!(parse_spec("a:1:/x=y/a").unwrap().component, "service");
    }

    #[test]
    fn specs_refuse_unknown_components_and_unsafe_ids() {
        for bad in [
            "binary=a:1:p",
            "a:1",
            "a::p",
            "../a:1:p",
            ".hidden:1:p",
            "dir/a:1:p",
            r"dir\a:1:p",
        ] {
            assert!(parse_spec(bad).is_err(), "{bad:?}");
        }
    }
}
