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
    let out = sign(&[]);
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
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out).contains("neither GGUF nor safetensors"),
        "{}",
        text(&out)
    );

    let _ = std::fs::remove_dir_all(&dir);
}
