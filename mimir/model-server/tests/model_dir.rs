//! A model on disk: a signed manifest file and the weights it names. The directory loads
//! only when the manifest's signature verifies and the weights stream to the signed
//! size, format and digest; an edited manifest, a changed byte, a truncated or swapped
//! file, another key or an unsupported family is refused. [AC-MI-7] [FR-MS-4] [NFR-PQ-3]
#![cfg(feature = "backend-fips")]

use std::path::PathBuf;

use model_server::{
    parse_manifest_file, parse_trust_key, render_manifest_file, sign_manifest, HwProfile, Licence,
    LicencePolicy, ModelLoader, ModelManifest, NodeProfile, WeightFormat, WeightsFile,
    MANIFEST_FILE, VELDRIS_MODEL_SIG_ALG,
};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::hash::sha256;
use pqc_agility::{OsRng, SignatureScheme, SigningKey, VerifyingKey};

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!("model-dir-{tag}-{}-{uniq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

fn keys() -> (VerifyingKey, SigningKey) {
    FipsSig::new(VELDRIS_MODEL_SIG_ALG)
        .keypair(&mut OsRng)
        .unwrap()
}

fn gguf() -> Vec<u8> {
    let mut bytes = b"GGUF\x03\x00\x00\x00".to_vec();
    bytes.extend((0..200_000u32).map(|i| (i.wrapping_mul(40_503) >> 8) as u8));
    bytes
}

fn manifest(family: &str, bytes: &[u8], name: &str) -> ModelManifest {
    ModelManifest {
        id: "granite-3.3-2b-instruct-q4km".into(),
        family: family.into(),
        version: "3.3".into(),
        sha256: sha256(bytes),
        hw: HwProfile::none(),
        licence: Licence {
            name: "Apache-2.0".into(),
            osi_approved: true,
            redistribution_ok: true,
        },
        weights: Some(WeightsFile {
            name: name.into(),
            format: WeightFormat::Gguf,
            quantisation: "Q4_K_M".into(),
            size: bytes.len() as u64,
        }),
    }
}

/// A signed model directory for `family`, and the key that trusts it.
fn model_dir(tag: &str, family: &str) -> (PathBuf, VerifyingKey, Vec<u8>) {
    let dir = scratch(tag);
    let bytes = gguf();
    let name = "granite-3.3-2b-instruct-Q4_K_M.gguf";
    std::fs::write(dir.join(name), &bytes).unwrap();
    let (vk, sk) = keys();
    let text = render_manifest_file(&sign_manifest(manifest(family, &bytes, name), &sk)).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), text).unwrap();
    (dir, vk, bytes)
}

fn node() -> NodeProfile {
    NodeProfile::cpu_only(4)
}

fn loader(vk: VerifyingKey) -> ModelLoader {
    ModelLoader::new(vk, LicencePolicy::default())
}

#[test]
fn a_signed_model_directory_loads_and_its_manifest_round_trips() {
    let (dir, vk, _) = model_dir("loads", "Granite");
    let model = loader(vk).verify_dir(&dir, &node()).unwrap();
    assert_eq!(model.loaded.family, "Granite");
    assert!(model
        .weights_path
        .ends_with("granite-3.3-2b-instruct-Q4_K_M.gguf"));

    let text = std::fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap();
    let parsed = parse_manifest_file(&text).unwrap();
    assert_eq!(parsed.manifest, model.artifact.manifest);
    assert_eq!(render_manifest_file(&parsed).unwrap(), text);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn changed_truncated_or_swapped_weights_are_refused() {
    let (dir, vk, bytes) = model_dir("weights", "Granite");
    let weights = dir.join("granite-3.3-2b-instruct-Q4_K_M.gguf");
    let loader = loader(vk);

    let mut changed = bytes.clone();
    changed[100_000] ^= 0x40;
    std::fs::write(&weights, &changed).unwrap();
    let e = loader.verify_dir(&dir, &node()).unwrap_err();
    assert!(e.contains("do not match the signed digest"), "{e}");

    std::fs::write(&weights, &bytes[..bytes.len() - 1]).unwrap();
    let e = loader.verify_dir(&dir, &node()).unwrap_err();
    assert!(e.contains("wrong size"), "{e}");

    // Same size, but a pickle rather than GGUF.
    let mut pickle = bytes.clone();
    pickle[..4].copy_from_slice(b"\x80\x04\x95\x10");
    std::fs::write(&weights, &pickle).unwrap();
    let e = loader.verify_dir(&dir, &node()).unwrap_err();
    assert!(e.contains("not in the signed format"), "{e}");

    std::fs::remove_file(&weights).unwrap();
    let e = loader.verify_dir(&dir, &node()).unwrap_err();
    assert!(e.contains("missing"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_edited_manifest_another_key_or_an_unknown_family_is_refused() {
    let (dir, vk, _) = model_dir("manifest", "Granite");
    let path = dir.join(MANIFEST_FILE);
    let text = std::fs::read_to_string(&path).unwrap();

    std::fs::write(
        &path,
        text.replace("quantisation Q4_K_M", "quantisation Q8_0"),
    )
    .unwrap();
    let e = loader(vk.clone()).verify_dir(&dir, &node()).unwrap_err();
    assert!(e.contains("signature does not verify"), "{e}");

    std::fs::write(
        &path,
        text.replace(
            "weights-file granite-3.3-2b-instruct-Q4_K_M.gguf",
            "weights-file ../../etc/passwd",
        ),
    )
    .unwrap();
    let e = loader(vk.clone()).verify_dir(&dir, &node()).unwrap_err();
    assert!(e.contains("plain file name"), "{e}");

    std::fs::write(&path, &text).unwrap();
    let (other, _) = keys();
    let e = loader(other).verify_dir(&dir, &node()).unwrap_err();
    assert!(e.contains("signature does not verify"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);

    let (dir, vk, _) = model_dir("family", "Llama");
    let e = loader(vk).verify_dir(&dir, &node()).unwrap_err();
    assert!(e.contains("family is not supported"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn only_gguf_and_safetensors_are_recognised() {
    assert_eq!(
        WeightFormat::sniff(b"GGUF\x03\x00\x00\x00"),
        Some(WeightFormat::Gguf)
    );
    let mut safetensors = 64u64.to_le_bytes().to_vec();
    safetensors.extend_from_slice(b"{\"__metadata__\":{}}");
    assert_eq!(
        WeightFormat::sniff(&safetensors),
        Some(WeightFormat::Safetensors)
    );
    for other in [
        &b"\x80\x04\x95\x10\x00\x00\x00\x00\x00"[..],
        b"PK\x03\x04\x14\x00\x00\x00\x08",
        b"GGU",
        b"",
    ] {
        assert_eq!(WeightFormat::sniff(other), None, "{other:?}");
    }
}

#[test]
fn manifest_files_and_trust_keys_parse_strictly() {
    let (_, sk) = keys();
    let bytes = gguf();
    let text =
        render_manifest_file(&sign_manifest(manifest("Granite", &bytes, "m.gguf"), &sk)).unwrap();
    assert!(parse_manifest_file(&text).is_ok());
    for (from, to) in [
        ("midgard-model-manifest 1\n", "midgard-model-manifest 2\n"),
        ("osi-approved yes", "osi-approved true"),
        ("min-gpus 0", "min-gpus +0"),
        ("weights-format gguf", "weights-format pickle"),
        ("version 3.3", "version 3 3"),
        ("family Granite\nversion 3.3", "version 3.3\nfamily Granite"),
    ] {
        let edited = text.replace(from, to);
        assert_ne!(edited, text);
        assert!(
            parse_manifest_file(&edited).is_err(),
            "{to:?} should not parse"
        );
    }
    assert!(parse_manifest_file(&format!("{text}extra line\n")).is_err());

    let hex = hex(keys().0.as_bytes());
    let line = format!("{} model-signing {hex}\n", VELDRIS_MODEL_SIG_ALG.as_str());
    assert!(parse_trust_key(&format!("# the model key\n{line}")).is_ok());
    assert!(parse_trust_key(&format!("{line}{line}")).is_err());
    assert!(parse_trust_key(&format!("ML-DSA-87 model-signing {hex}")).is_err());
    assert!(parse_trust_key("").is_err());
}
