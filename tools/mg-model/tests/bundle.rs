//! `mg-model pack` and `unpack`: a verified model directory crosses the air gap as one
//! signed, streamed bundle and comes out verified. An unverified model is never packed;
//! a damaged or wrongly signed bundle is never unpacked; and a bundle whose model fails
//! the load gate leaves nothing behind. [AC-MS-3] [FR-B-5] [AC-MI-7]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use attest::Keystore;
use pqc_agility::{OsRng, SigId};

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "mg-model-bundle-{tag}-{}-{uniq}",
        std::process::id()
    ));
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

/// A model signed with a fresh model key: (model directory, its trust file, weights).
fn signed_model(dir: &Path, name: &str) -> (PathBuf, PathBuf, Vec<u8>) {
    let keys = dir.join(format!("{name}-keys"));
    assert!(mg_model(&["keygen", "--out", s(&keys)]).status.success());
    let model = dir.join(name);
    std::fs::create_dir_all(&model).unwrap();
    let weights_path = model.join("tiny-Q4_K_M.gguf");
    let mut weights = b"GGUF\x03\x00\x00\x00".to_vec();
    weights.extend((0..3_000_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8));
    std::fs::write(&weights_path, &weights).unwrap();
    let keystore = keys.join("model-signing.keystore");
    let out = mg_model(&[
        "sign",
        "--weights",
        s(&weights_path),
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
    ]);
    assert!(out.status.success(), "{}", text(&out));
    (model, keys.join("model-signing.pub"), weights)
}

/// A Bifrost admission keystore (ML-DSA-65) and its anchor file.
fn admission(dir: &Path, name: &str) -> (PathBuf, PathBuf) {
    let key = Keystore::generate(name, SigId::MlDsa65, &mut OsRng).unwrap();
    let keystore = dir.join(format!("{name}.keystore"));
    let anchor = dir.join(format!("{name}.pub"));
    key.write(&keystore).unwrap();
    std::fs::write(&anchor, key.export_verifying_key()).unwrap();
    (keystore, anchor)
}

#[test]
fn a_verified_model_crosses_as_one_bundle_and_comes_out_verified() {
    let dir = scratch("roundtrip");
    let (model, trust, weights) = signed_model(&dir, "model");
    let (keystore, anchor) = admission(&dir, "bifrost-admission");
    let bundle = dir.join("tiny.mbundle");

    let pack = || {
        mg_model(&[
            "pack",
            "--dir",
            s(&model),
            "--model-trust",
            s(&trust),
            "--keystore",
            s(&keystore),
            "--out",
            s(&bundle),
        ])
    };
    let out = pack();
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("packed tiny-q4km (Granite)"),
        "{}",
        text(&out)
    );
    assert_eq!(
        pack().status.code(),
        Some(1),
        "pack must never overwrite a bundle"
    );

    let unpacked = dir.join("unpacked");
    let unpack = || {
        mg_model(&[
            "unpack",
            "--bundle",
            s(&bundle),
            "--bundle-trust",
            s(&anchor),
            "--model-trust",
            s(&trust),
            "--out",
            s(&unpacked),
        ])
    };
    let out = unpack();
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("unpacked and verified tiny-q4km (Granite)"),
        "{}",
        text(&out)
    );
    assert_eq!(
        std::fs::read(unpacked.join("tiny-Q4_K_M.gguf")).unwrap(),
        weights
    );
    assert_eq!(
        std::fs::read(unpacked.join("model.manifest")).unwrap(),
        std::fs::read(model.join("model.manifest")).unwrap()
    );
    assert_eq!(
        unpack().status.code(),
        Some(1),
        "unpack writes a new directory only"
    );

    // The unpacked directory is a model the plane loads.
    let out = mg_model(&["verify", "--dir", s(&unpacked), "--trust", s(&trust)]);
    assert!(out.status.success(), "{}", text(&out));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_unverified_is_packed_or_left_behind() {
    let dir = scratch("refused");
    let (model, trust, _) = signed_model(&dir, "model");
    let (_, other_trust, _) = signed_model(&dir, "other");
    let (keystore, anchor) = admission(&dir, "bifrost-admission");
    let (_, other_anchor) = admission(&dir, "someone-else");

    // A model that fails the load gate is never packed.
    let bundle = dir.join("tiny.mbundle");
    let out = mg_model(&[
        "pack",
        "--dir",
        s(&model),
        "--model-trust",
        s(&other_trust),
        "--keystore",
        s(&keystore),
        "--out",
        s(&bundle),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out).contains("signature does not verify"),
        "{}",
        text(&out)
    );
    assert!(!bundle.exists());

    let out = mg_model(&[
        "pack",
        "--dir",
        s(&model),
        "--model-trust",
        s(&trust),
        "--keystore",
        s(&keystore),
        "--out",
        s(&bundle),
    ]);
    assert!(out.status.success(), "{}", text(&out));

    let unpacked = dir.join("unpacked");
    let unpack = |bundle: &Path, anchor: &Path, trust: &Path| {
        mg_model(&[
            "unpack",
            "--bundle",
            s(bundle),
            "--bundle-trust",
            s(anchor),
            "--model-trust",
            s(trust),
            "--out",
            s(&unpacked),
        ])
    };

    // Another admission key's anchor.
    let out = unpack(&bundle, &other_anchor, &trust);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("REJECTED"), "{}", text(&out));
    assert!(!unpacked.exists());

    // One changed byte in the weights, inside the bundle.
    let mut bytes = std::fs::read(&bundle).unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 0x01;
    let damaged = dir.join("damaged.mbundle");
    std::fs::write(&damaged, bytes).unwrap();
    let out = unpack(&damaged, &anchor, &trust);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("damaged"), "{}", text(&out));
    assert!(!unpacked.exists());

    // A bundle that verifies, carrying a model this enclosure does not trust: the load
    // gate refuses it after unpacking, and the directory is removed.
    let out = unpack(&bundle, &anchor, &other_trust);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("was removed"), "{}", text(&out));
    assert!(!unpacked.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
