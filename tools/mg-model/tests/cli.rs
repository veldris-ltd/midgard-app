//! `mg-model` end to end: a key is generated once, weights are signed only when they
//! are GGUF or safetensors and match the published digest, and `verify` accepts the
//! signed directory and refuses a changed byte or another key. [AC-MI-7] [FR-MS-4]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pqc_agility::hash::sha256;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!("mg-model-{tag}-{}-{uniq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn mg_model(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mg-model"))
        .env(
            "MIDGARD_KEYSTORE_PASSPHRASE_FILE",
            attest::keystore::test_support::shared_test_passphrase_file(),
        )
        .args(args)
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
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

#[test]
fn keygen_sign_verify_and_refuse_tampering() {
    let dir = scratch("flow");
    let keys = dir.join("keys");
    let model = dir.join("model");
    std::fs::create_dir_all(&model).unwrap();
    let weights = model.join("tiny-Q4_K_M.gguf");
    let mut bytes = b"GGUF\x03\x00\x00\x00".to_vec();
    bytes.extend((0..3_000_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8));
    std::fs::write(&weights, &bytes).unwrap();
    let digest = hex(&sha256(&bytes));

    let out = mg_model(&["keygen", "--out", s(&keys)]);
    assert!(out.status.success(), "{}", text(&out));
    let again = mg_model(&["keygen", "--out", s(&keys)]);
    assert_eq!(
        again.status.code(),
        Some(1),
        "keygen must never overwrite a key"
    );

    let keystore = keys.join("model-signing.keystore");
    let sign = |extra: &[&str]| {
        let mut args = vec![
            "sign",
            "--weights",
            s(&weights),
            "--id",
            "tiny-q4km",
            "--family",
            "Granite",
            "--version",
            "1",
            "--quantisation",
            "Q4_K_M",
            "--licence",
            "Apache-2.0",
            "--osi-approved",
            "--redistributable",
            "--keystore",
            s(&keystore),
        ];
        args.extend_from_slice(extra);
        mg_model(&args)
    };

    let wrong = "00".repeat(32);
    let out = sign(&["--sha256", &wrong]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("nothing signed"));
    assert!(!model.join("model.manifest").exists());

    let out = sign(&["--sha256", &digest]);
    assert!(out.status.success(), "{}", text(&out));
    let out = sign(&["--sha256", &digest]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "sign must never overwrite a manifest"
    );

    let trust = keys.join("model-signing.pub");
    let out = mg_model(&["verify", "--dir", s(&model), "--trust", s(&trust)]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("verified tiny-q4km (Granite)"),
        "{}",
        text(&out)
    );
    assert!(text(&out).contains(&digest));

    // One changed byte in the weights.
    let mut tampered = bytes.clone();
    tampered[1_500_000] ^= 1;
    std::fs::write(&weights, &tampered).unwrap();
    let out = mg_model(&["verify", "--dir", s(&model), "--trust", s(&trust)]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out).contains("do not match the signed digest"),
        "{}",
        text(&out)
    );
    std::fs::write(&weights, &bytes).unwrap();

    // Another key.
    let other = dir.join("other-keys");
    assert!(mg_model(&["keygen", "--out", s(&other)]).status.success());
    let out = mg_model(&[
        "verify",
        "--dir",
        s(&model),
        "--trust",
        s(&other.join("model-signing.pub")),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out).contains("signature does not verify"),
        "{}",
        text(&out)
    );

    // A pickled checkpoint is never signed.
    let pickle = dir.join("pickle").join("model.bin");
    std::fs::create_dir_all(pickle.parent().unwrap()).unwrap();
    std::fs::write(
        &pickle,
        b"\x80\x04\x95\x10\x00\x00\x00\x00\x00\x00\x00}\x94.",
    )
    .unwrap();
    let out = mg_model(&[
        "sign",
        "--weights",
        s(&pickle),
        "--id",
        "evil",
        "--family",
        "Granite",
        "--version",
        "1",
        "--quantisation",
        "none",
        "--licence",
        "MIT",
        "--keystore",
        s(&keystore),
        "--sha256",
        &wrong,
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out).contains("neither GGUF nor safetensors"),
        "{}",
        text(&out)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Tiny GGUF weights in `dir/model`, and their SHA-256 in hex.
fn tiny_model(dir: &Path) -> (PathBuf, PathBuf, String) {
    let model = dir.join("model");
    std::fs::create_dir_all(&model).unwrap();
    let weights = model.join("tiny-Q4_K_M.gguf");
    let mut bytes = b"GGUF\x03\x00\x00\x00".to_vec();
    bytes.extend((0..65_536u32).map(|i| (i % 251) as u8));
    std::fs::write(&weights, &bytes).unwrap();
    (model, weights, hex(&sha256(&bytes)))
}

fn sign_args<'a>(weights: &'a Path, keystore: &'a Path) -> Vec<&'a str> {
    vec![
        "sign",
        "--weights",
        s(weights),
        "--id",
        "tiny-q4km",
        "--family",
        "Granite",
        "--version",
        "1",
        "--quantisation",
        "Q4_K_M",
        "--licence",
        "Apache-2.0",
        "--osi-approved",
        "--redistributable",
        "--keystore",
        s(keystore),
    ]
}

/// A download whose digest was never checked is never signed: `sign` without `--sha256`
/// is a usage error, and no manifest is written. [VLD-ANB-MIDGARD-0028] [AC-66.3]
#[test]
fn sign_without_sha256_is_a_usage_error() {
    let dir = scratch("no-sha256");
    let keys = dir.join("keys");
    assert!(mg_model(&["keygen", "--out", s(&keys)]).status.success());
    let (model, weights, digest) = tiny_model(&dir);
    let keystore = keys.join("model-signing.keystore");

    let out = mg_model(&sign_args(&weights, &keystore));
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("--sha256"), "{}", text(&out));
    assert!(!model.join("model.manifest").exists(), "nothing is signed");

    // With the published digest, it signs.
    let mut args = sign_args(&weights, &keystore);
    args.extend(["--sha256", &digest]);
    let out = mg_model(&args);
    assert!(out.status.success(), "{}", text(&out));
    assert!(model.join("model.manifest").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `verify` refuses a trust file inside the model directory, as mimir-server does: the
/// anchor and the weights it vouches for never share a writable location. The same key
/// outside the directory verifies. [VLD-ANB-MIDGARD-0028] [AC-66.1]
#[test]
fn verify_refuses_a_trust_file_inside_the_model_dir() {
    let dir = scratch("trust-inside");
    let keys = dir.join("keys");
    assert!(mg_model(&["keygen", "--out", s(&keys)]).status.success());
    let (model, weights, digest) = tiny_model(&dir);
    let keystore = keys.join("model-signing.keystore");
    let mut args = sign_args(&weights, &keystore);
    args.extend(["--sha256", &digest]);
    assert!(mg_model(&args).status.success());
    let trust = keys.join("model-signing.pub");
    let inside = model.join("model-signing.pub");
    std::fs::copy(&trust, &inside).unwrap();

    let out = mg_model(&["verify", "--dir", s(&model), "--trust", s(&inside)]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(
        text(&out).contains("must not live inside the model directory"),
        "{}",
        text(&out)
    );
    let out = mg_model(&["verify", "--dir", s(&model), "--trust", s(&trust)]);
    assert!(out.status.success(), "{}", text(&out));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `keygen` writes the model-signing key as a keystore version 2, its custody root
/// wrapped under the keystore passphrase, read back through the same Keystore API; a
/// wrong passphrase does not open it, and keygen refuses to run without a passphrase.
/// [VLD-ANB-MIDGARD-0028] [VLD-ANB-MIDGARD-0007] [AC-66.5]
#[test]
fn keygen_writes_keystore_v2_under_a_passphrase() {
    use attest::keystore::test_support::test_passphrase;
    use attest::keystore::{format_version, VERSION};
    use attest::{Keystore, RootWrap};
    use pqc_agility::SecretBytes;

    let dir = scratch("keygen-v2");
    let keys = dir.join("keys");
    let out = mg_model(&["keygen", "--out", s(&keys)]);
    assert!(out.status.success(), "{}", text(&out));
    let path = keys.join("model-signing.keystore");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(format_version(&bytes), Some(VERSION));
    assert_eq!(VERSION, 2);

    let keystore = Keystore::read(&path, &RootWrap::Passphrase(&test_passphrase())).unwrap();
    assert_eq!(keystore.alg, model_server::VELDRIS_MODEL_SIG_ALG);
    // The public half is what keygen exported beside it.
    assert_eq!(
        std::fs::read_to_string(keys.join("model-signing.pub")).unwrap(),
        keystore.export_verifying_key()
    );
    let wrong = SecretBytes::new(b"not-the-passphrase-at-all".to_vec());
    assert!(Keystore::read(&path, &RootWrap::Passphrase(&wrong)).is_err());

    // No passphrase, no key.
    let bare = dir.join("bare");
    let out = Command::new(env!("CARGO_BIN_EXE_mg-model"))
        .env_remove("MIDGARD_KEYSTORE_PASSPHRASE_FILE")
        .args(["keygen", "--out", s(&bare)])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("MIDGARD_KEYSTORE_PASSPHRASE_FILE"),
        "{}",
        text(&out)
    );
    assert!(!bare.join("model-signing.keystore").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
