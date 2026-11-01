//! A bundle's signature is checked over its manifest's bytes exactly as they lie in the
//! file, before the manifest is decoded: a tampered manifest is refused as a bad
//! signature even when it is also something no decoder would accept. And a packer
//! handed open payloads packs what it was handed, never whatever the path names by
//! then, refusing a payload that is not the one that was verified. [MG-SEC-14] [AC-B-3]
#![cfg(feature = "backend-fips")]

use std::io::Cursor;
use std::path::PathBuf;

use bifrost_artefact::{build_and_sign, generate_sbom, SbomComponent};
use bifrost_bundle::{
    build_bundle, import_streaming, pack_streaming_open, read_verified_bundle, verify_bundle_bytes,
    write_bundle, BundleError, OpenArtefact,
};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::hash::sha256;
use pqc_agility::{OsRng, SignatureScheme, SigningKey, VerifyingKey};

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir =
        std::env::temp_dir().join(format!("bundle-first-{tag}-{}-{uniq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn keys() -> (VerifyingKey, SigningKey) {
    FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
        .keypair(&mut OsRng)
        .unwrap()
}

fn sbom(id: &str) -> String {
    generate_sbom(
        id,
        "3.3",
        "model-bundle",
        vec![SbomComponent::new(id, "3.3")],
    )
}

/// The offset of the bundle id's length prefix: after the magic, the version and the
/// signature (its algorithm byte, length and bytes).
fn manifest_at(file: &[u8]) -> usize {
    let sig_len = u32::from_le_bytes(file[10..14].try_into().unwrap()) as usize;
    14 + sig_len
}

/// Tamper with a bundle's manifest so that, besides breaking the signature, decoding it
/// would fail: the bundle id's bytes become invalid UTF-8, and then its length claims
/// far more than the file holds.
fn tampered(file: &[u8]) -> Vec<Vec<u8>> {
    let at = manifest_at(file);
    let mut not_utf8 = file.to_vec();
    not_utf8[at + 4] = 0xff;
    let mut oversized = file.to_vec();
    oversized[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    vec![not_utf8, oversized]
}

/// A refusal that came before the manifest was decoded: the signature's, or the framing's
/// (a length that runs past the file or the manifest limit) — never a complaint about
/// what a field contains.
fn refused_unread(e: &BundleError) {
    match e {
        BundleError::BadBundleSignature => {}
        BundleError::Io(m) if m.contains("oversized") || m.contains("truncated") => {}
        other => panic!("decoded before verifying: {other}"),
    }
}

#[test]
fn a_tampered_manifest_is_refused_at_the_signature_before_it_is_decoded() {
    let dir = scratch("tamper");
    let (vk, sk) = keys();

    // Version 2, streamed.
    let weights = dir.join("w.bin");
    std::fs::write(&weights, vec![7u8; 100_000]).unwrap();
    let mut file = std::fs::File::open(&weights).unwrap();
    let streamed = dir.join("v2.mbundle");
    pack_streaming_open(
        &streamed,
        "b",
        "l",
        &mut [OpenArtefact {
            id: "w.bin".into(),
            version: "3.3".into(),
            component: "model-bundle".into(),
            sbom: sbom("w.bin"),
            payload: &mut file,
            expect_sha256: None,
        }],
        &sk,
        16 * 1024,
    )
    .unwrap();

    // Version 1, whole.
    let artefact = build_and_sign("a", "3.3", "service", b"payload".to_vec(), sbom("a"), &sk);
    let whole = dir.join("v1.mbundle");
    write_bundle(&whole, &build_bundle("b", "l", vec![artefact], &sk)).unwrap();

    for path in [&streamed, &whole] {
        let good = std::fs::read(path).unwrap();
        assert!(verify_bundle_bytes(&good, &vk).is_ok());
        assert!(import_streaming(path, &vk, None).is_ok());
        for (n, bad) in tampered(&good).into_iter().enumerate() {
            let e = verify_bundle_bytes(&bad, &vk).unwrap_err();
            refused_unread(&e);
            if n == 0 {
                // Framing intact: the signature itself refuses it.
                assert_eq!(e, BundleError::BadBundleSignature, "{}", path.display());
            }
            let tampered_path = dir.join("tampered.mbundle");
            std::fs::write(&tampered_path, &bad).unwrap();
            refused_unread(&import_streaming(&tampered_path, &vk, None).unwrap_err());
            refused_unread(&read_verified_bundle(&tampered_path, &vk).unwrap_err());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_packer_packs_the_handle_it_was_given_and_refuses_an_unverified_payload() {
    let dir = scratch("handle");
    let (vk, sk) = keys();
    let path = dir.join("weights.gguf");
    let verified = vec![1u8; 300_000];
    std::fs::write(&path, &verified).unwrap();
    let mut file = std::fs::File::open(&path).unwrap();

    // The path is swapped after the file was opened (and, say, verified).
    let evil = dir.join("evil.gguf");
    std::fs::write(&evil, vec![2u8; 300_000]).unwrap();
    std::fs::rename(&evil, &path).unwrap();

    let manifest_bytes = b"signed manifest bytes".to_vec();
    let mut manifest = Cursor::new(manifest_bytes.clone());
    let bundle = dir.join("m.mbundle");
    pack_streaming_open(
        &bundle,
        "m",
        "m@3.3",
        &mut [
            OpenArtefact {
                id: "model.manifest".into(),
                version: "3.3".into(),
                component: "model-bundle".into(),
                sbom: sbom("model.manifest"),
                payload: &mut manifest,
                expect_sha256: Some(sha256(&manifest_bytes)),
            },
            OpenArtefact {
                id: "weights.gguf".into(),
                version: "3.3".into(),
                component: "model-bundle".into(),
                sbom: sbom("weights.gguf"),
                payload: &mut file,
                expect_sha256: Some(sha256(&verified)),
            },
        ],
        &sk,
        64 * 1024,
    )
    .unwrap();
    let out = dir.join("out");
    import_streaming(&bundle, &vk, Some(&out)).unwrap();
    assert_eq!(std::fs::read(out.join("weights.gguf")).unwrap(), verified);
    assert_eq!(
        std::fs::read(out.join("model.manifest")).unwrap(),
        manifest_bytes
    );

    // A payload that is not the verified one is refused, and nothing is written.
    let mut swapped = std::fs::File::open(&path).unwrap();
    let refused = dir.join("refused.mbundle");
    let e = pack_streaming_open(
        &refused,
        "m",
        "m@3.3",
        &mut [OpenArtefact {
            id: "weights.gguf".into(),
            version: "3.3".into(),
            component: "model-bundle".into(),
            sbom: sbom("weights.gguf"),
            payload: &mut swapped,
            expect_sha256: Some(sha256(&verified)),
        }],
        &sk,
        64 * 1024,
    )
    .unwrap_err();
    assert!(
        e.to_string().contains("not the payload that was verified"),
        "{e}"
    );
    assert!(!refused.exists());
    assert!(!refused.with_extension("tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
