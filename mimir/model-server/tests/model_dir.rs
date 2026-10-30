//! A model on disk: a signed manifest file and the weights it names. The directory loads
//! only when the manifest's signature verifies and the weights stream to the signed
//! size, format and digest; an edited manifest, a changed byte, a truncated or swapped
//! file, another key or an unsupported family is refused. [AC-MI-7] [FR-MS-4] [NFR-PQ-3]
//!
//! The signature is checked over the manifest file's raw bytes before it is parsed; an
//! older release than the one last accepted is refused; and the weights are read through
//! the one handle they were hashed through. [MG-SEC-14]
#![cfg(feature = "backend-fips")]

use std::io::Read;
use std::path::PathBuf;

use attest::{ReleaseFloor, RollbackError};
use model_server::{
    manifest_file_with_signature, parse_trust_key, render_manifest_body, sign_manifest_file,
    verify_manifest_file, HwProfile, Licence, LicencePolicy, ManifestFileError, ModelLoader,
    ModelManifest, NodeProfile, WeightFormat, WeightsFile, MANIFEST_FILE, VELDRIS_MODEL_SIG_ALG,
};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::hash::sha256;
use pqc_agility::{OsRng, SignatureScheme, SigningKey, VerifyingKey};

const WEIGHTS: &str = "granite-3.3-2b-instruct-Q4_K_M.gguf";

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
    gguf_seeded(40_503)
}

fn gguf_seeded(seed: u32) -> Vec<u8> {
    let mut bytes = b"GGUF\x03\x00\x00\x00".to_vec();
    bytes.extend((0..200_000u32).map(|i| (i.wrapping_mul(seed) >> 8) as u8));
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

/// Write `weights` and a manifest for them, signed by `sk`, into `dir`.
fn write_model(dir: &std::path::Path, m: &ModelManifest, weights: &[u8], sk: &SigningKey) {
    let name = &m.weights.as_ref().unwrap().name;
    std::fs::write(dir.join(name), weights).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), sign_manifest_file(m, sk).unwrap()).unwrap();
}

/// A signed model directory for `family`, and the key that trusts it.
fn model_dir(tag: &str, family: &str) -> (PathBuf, VerifyingKey, Vec<u8>) {
    let dir = scratch(tag);
    let bytes = gguf();
    let (vk, sk) = keys();
    write_model(&dir, &manifest(family, &bytes, WEIGHTS), &bytes, &sk);
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
    let model = loader(vk.clone()).verify_dir(&dir, &node()).unwrap();
    assert_eq!(model.loaded.family, "Granite");
    assert!(model.weights.path().ends_with(WEIGHTS));

    let text = std::fs::read(dir.join(MANIFEST_FILE)).unwrap();
    assert_eq!(model.manifest_bytes, text);
    let parsed = verify_manifest_file(&text, &vk).unwrap();
    assert_eq!(parsed, model.manifest);
    assert!(String::from_utf8(text)
        .unwrap()
        .starts_with(&render_manifest_body(&parsed).unwrap()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn changed_truncated_or_swapped_weights_are_refused() {
    let (dir, vk, bytes) = model_dir("weights", "Granite");
    let weights = dir.join(WEIGHTS);
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

/// A tampered manifest is refused at its signature, before it is parsed: even an edit
/// the parser would also reject, or bytes that are no manifest at all, fail as a bad
/// signature, never as a parse error. [MG-SEC-14]
#[test]
fn a_tampered_manifest_is_refused_before_it_is_parsed() {
    let (dir, vk, _) = model_dir("manifest", "Granite");
    let path = dir.join(MANIFEST_FILE);
    let text = std::fs::read_to_string(&path).unwrap();
    let signature_line = &text[text.rfind("signature ").unwrap()..];

    for tampered in [
        text.replace("quantisation Q4_K_M", "quantisation Q8_0"),
        // The parser would refuse this path, but it never sees it.
        text.replace(
            &format!("weights-file {WEIGHTS}"),
            "weights-file ../../etc/passwd",
        ),
        // No manifest at all under a real signature line.
        format!("\u{0}\u{ff}garbage that no parser would accept\n{signature_line}"),
    ] {
        assert_eq!(
            verify_manifest_file(tampered.as_bytes(), &vk),
            Err(ManifestFileError::BadSignature),
            "{tampered:?}"
        );
        std::fs::write(&path, &tampered).unwrap();
        let e = loader(vk.clone()).verify_dir(&dir, &node()).unwrap_err();
        assert!(e.contains("signature does not verify"), "{e}");
        assert!(
            !e.contains("plain file name"),
            "parsed before verifying: {e}"
        );
    }

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
fn a_format_1_or_unsigned_manifest_is_refused() {
    let (_, sk) = keys();
    let bytes = gguf();
    let text = sign_manifest_file(&manifest("Granite", &bytes, "m.gguf"), &sk).unwrap();
    let vk = keys().0;
    let legacy = text.replace("midgard-model-manifest 2\n", "midgard-model-manifest 1\n");
    assert!(matches!(
        verify_manifest_file(legacy.as_bytes(), &vk),
        Err(ManifestFileError::Unsigned(why)) if why.contains("format 1")
    ));
    let unsigned = &text[..text.rfind("signature ").unwrap()];
    assert!(matches!(
        verify_manifest_file(unsigned.as_bytes(), &vk),
        Err(ManifestFileError::Unsigned(_))
    ));
    assert!(matches!(
        verify_manifest_file(format!("{text}extra line\n").as_bytes(), &vk),
        Err(ManifestFileError::Unsigned(_))
    ));
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

/// A body that verifies is still parsed strictly: each malformed body below is signed by
/// the trusted key, so it reaches the parser, which refuses it.
#[test]
fn signed_manifest_bodies_and_trust_keys_parse_strictly() {
    let (vk, sk) = keys();
    let bytes = gguf();
    let body = render_manifest_body(&manifest("Granite", &bytes, "m.gguf")).unwrap();
    let sign = |body: &str| {
        let sig =
            attest::accept::sign_in_domain(&attest::accept::MIMIR_MODEL_FILE, body.as_bytes(), &sk)
                .unwrap();
        manifest_file_with_signature(body, &sig)
    };
    assert!(verify_manifest_file(sign(&body).as_bytes(), &vk).is_ok());
    for (from, to) in [
        ("midgard-model-manifest 2\n", "midgard-model-manifest 3\n"),
        ("osi-approved yes", "osi-approved true"),
        ("min-gpus 0", "min-gpus +0"),
        ("weights-format gguf", "weights-format pickle"),
        ("version 3.3", "version 3 3"),
        ("family Granite\nversion 3.3", "version 3.3\nfamily Granite"),
        ("weights-file m.gguf", "weights-file ../m.gguf"),
    ] {
        let edited = body.replace(from, to);
        assert_ne!(edited, body);
        assert!(
            matches!(
                verify_manifest_file(sign(&edited).as_bytes(), &vk),
                Err(ManifestFileError::Malformed(_))
            ),
            "{to:?} should not parse"
        );
    }
    assert!(matches!(
        verify_manifest_file(sign(&format!("{body}extra line\n")).as_bytes(), &vk),
        Err(ManifestFileError::Malformed(_))
    ));

    let hex = hex(keys().0.as_bytes());
    let line = format!("{} model-signing {hex}\n", VELDRIS_MODEL_SIG_ALG.as_str());
    assert!(parse_trust_key(&format!("# the model key\n{line}")).is_ok());
    assert!(parse_trust_key(&format!("{line}{line}")).is_err());
    assert!(parse_trust_key(&format!("ML-DSA-87 model-signing {hex}")).is_err());
    assert!(parse_trust_key("").is_err());
}

/// Anti-rollback: once a release is accepted, an older signed release of the same model
/// is refused, and so is the same version with different signed content — across a
/// restart, because the floor is persisted. [MG-SEC-14]
#[test]
fn an_older_signed_release_is_refused() {
    let dir = scratch("rollback");
    let floor_path = dir.join("model-release.floor");
    let model = dir.join("model");
    std::fs::create_dir_all(&model).unwrap();
    let (vk, sk) = keys();
    let loader = loader(vk);
    let release = |version: &str, weights: &[u8]| {
        let mut m = manifest("Granite", weights, WEIGHTS);
        m.version = version.into();
        write_model(&model, &m, weights, &sk);
    };
    let load = |floor: &mut ReleaseFloor| loader.verify_dir_at_floor(&model, &node(), floor);

    let (v33, v32, other) = (gguf(), gguf_seeded(7), gguf_seeded(11));
    let mut floor = ReleaseFloor::open(&floor_path).unwrap();
    release("3.3", &v33);
    load(&mut floor).unwrap();
    // The same release again — a restart — loads.
    load(&mut floor).unwrap();

    // An older release, validly signed by the same key, is refused.
    release("3.2", &v32);
    let e = load(&mut floor).unwrap_err();
    assert!(e.contains("older than 3.3"), "{e}");
    // A plain verify still checks it: only the serving path keeps a floor.
    assert!(loader.verify_dir(&model, &node()).is_ok());

    // The same version with different signed content is equivocation.
    release("3.3", &other);
    let e = load(&mut floor).unwrap_err();
    assert!(e.contains("different signed content"), "{e}");

    // A restart does not forget the floor.
    let mut reopened = ReleaseFloor::open(&floor_path).unwrap();
    release("3.2", &v32);
    assert!(load(&mut reopened).is_err());
    assert!(matches!(
        reopened.check("granite-3.3-2b-instruct-q4km", "3.2", &[0u8; 32]),
        Err(RollbackError::Older { .. })
    ));

    // A newer release raises the floor.
    release("3.4", &other);
    load(&mut reopened).unwrap();
    release("3.3", &v33);
    assert!(load(&mut reopened).unwrap_err().contains("older than 3.4"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The weights are hashed through one handle and read back through it: a file swapped
/// in at the path after the check is never what is read, and the swap is detected.
/// [MG-SEC-14]
#[test]
fn weights_swapped_after_the_check_are_never_read() {
    let (dir, vk, bytes) = model_dir("swap", "Granite");
    let mut model = loader(vk).verify_dir(&dir, &node()).unwrap();
    assert!(model.weights.still_at_path());

    // Swap other bytes of the same size in at the same path.
    let mut evil = bytes.clone();
    evil[50_000..50_100].fill(0x41);
    let staged = dir.join("evil.gguf");
    std::fs::write(&staged, &evil).unwrap();
    std::fs::rename(&staged, dir.join(WEIGHTS)).unwrap();

    // Reading through the verified handle still yields the signed bytes...
    let mut read = Vec::new();
    model
        .weights
        .file()
        .unwrap()
        .read_to_end(&mut read)
        .unwrap();
    assert_eq!(sha256(&read), model.manifest.sha256);
    // ...twice over: the handle rewinds for each reader.
    let mut again = Vec::new();
    model
        .weights
        .file()
        .unwrap()
        .read_to_end(&mut again)
        .unwrap();
    assert_eq!(read, again);
    // And the path no longer names the verified file.
    assert!(!model.weights.still_at_path());
    let _ = std::fs::remove_dir_all(&dir);
}
