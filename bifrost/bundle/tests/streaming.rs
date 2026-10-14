//! Streaming bundles (format version 2): artefacts packed and imported a chunk at a
//! time, so a model's weights can cross the air gap without being held in memory. A
//! round trip writes out exactly what was packed; a damaged chunk is located; a forged
//! chunk with a rewritten digest is still refused by the signed payload digest; a wrong
//! key, a truncated file, trailing bytes and an existing file are refused; and a refused
//! import leaves nothing behind. Version 1 bundles and the in-memory reader keep
//! working. [FR-B-5] [AC-B-3] [AC-MS-3]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use bifrost_artefact::{build_and_sign, generate_sbom, SbomComponent, VerifyError};
use bifrost_bundle::{
    build_bundle, import, import_streaming, pack_streaming, read_bundle, write_bundle, BundleError,
    FileArtefact,
};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::hash::sha256;
use pqc_agility::{OsRng, SignatureScheme, SigningKey, VerifyingKey};

/// Small enough that the weights span many chunks.
const CHUNK: u32 = 64 * 1024;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir =
        std::env::temp_dir().join(format!("bundle-stream-{tag}-{}-{uniq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn keys() -> (VerifyingKey, SigningKey) {
    FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
        .keypair(&mut OsRng)
        .unwrap()
}

/// Deterministic bytes with no repeating 64-byte window.
fn pattern(len: usize, seed: u32) -> Vec<u8> {
    (0..len as u32)
        .map(|i| {
            let x = i.wrapping_mul(2_654_435_761).wrapping_add(seed);
            (x ^ (x >> 15) ^ (x >> 23)) as u8
        })
        .collect()
}

/// Weights spanning nine full chunks and part of a tenth, a small manifest, and an
/// empty file, each on disk.
fn sources(dir: &Path) -> Vec<(FileArtefact, Vec<u8>)> {
    std::fs::create_dir_all(dir).unwrap();
    let files = [
        (
            "granite.gguf",
            "model-bundle",
            pattern(CHUNK as usize * 9 + 12_345, 7),
        ),
        (
            "model.manifest",
            "model-bundle",
            b"midgard-model-manifest 1\n".to_vec(),
        ),
        ("empty.bin", "tool", Vec::new()),
    ];
    files
        .into_iter()
        .map(|(id, component, payload)| {
            let path = dir.join(id);
            std::fs::write(&path, &payload).unwrap();
            let sbom = generate_sbom(id, "3.3", component, vec![SbomComponent::new(id, "3.3")]);
            (
                FileArtefact {
                    id: id.into(),
                    version: "3.3".into(),
                    component: component.into(),
                    sbom,
                    path,
                },
                payload,
            )
        })
        .collect()
}

fn packed(dir: &Path, sk: &SigningKey) -> (PathBuf, Vec<(FileArtefact, Vec<u8>)>) {
    let sources = sources(&dir.join("src"));
    let list: Vec<FileArtefact> = sources.iter().map(|(a, _)| a.clone()).collect();
    let bundle = dir.join("granite.mbundle");
    pack_streaming(&bundle, "granite-3.3-2b", "v3.3", &list, sk, CHUNK).unwrap();
    (bundle, sources)
}

/// Where the fourth chunk of the weights starts in the bundle file.
fn fourth_chunk_at(file: &[u8], weights: &[u8]) -> usize {
    let start = &weights[3 * CHUNK as usize..3 * CHUNK as usize + 64];
    file.windows(64)
        .position(|w| w == start)
        .expect("the fourth chunk is in the file")
}

#[test]
fn a_streamed_bundle_imports_exactly_what_was_packed() {
    let dir = scratch("roundtrip");
    let (vk, sk) = keys();
    let (bundle, sources) = packed(&dir, &sk);

    let out = dir.join("out");
    let refs = import_streaming(&bundle, &vk, Some(&out)).unwrap();
    let ids: Vec<&str> = refs.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["granite.gguf", "model.manifest", "empty.bin"]);
    for (artefact, payload) in &sources {
        assert_eq!(&std::fs::read(out.join(&artefact.id)).unwrap(), payload);
        assert_eq!(
            std::fs::read_to_string(out.join(format!("{}.sbom.cdx.json", artefact.id))).unwrap(),
            artefact.sbom
        );
    }
    assert!(!dir.join(".out.partial").exists(), "staging left behind");

    // Verify-only writes nothing.
    assert_eq!(import_streaming(&bundle, &vk, None).unwrap(), refs);

    // The in-memory reader reads version 2 too, and its import agrees.
    let whole = read_bundle(&bundle).unwrap();
    assert_eq!(import(&vk, &whole).unwrap(), refs);
    assert_eq!(whole.artefacts[0].payload, sources[0].1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn damage_is_located_and_a_forged_chunk_is_still_refused() {
    let dir = scratch("damage");
    let (vk, sk) = keys();
    let (bundle, sources) = packed(&dir, &sk);
    let original = std::fs::read(&bundle).unwrap();
    let at = fourth_chunk_at(&original, &sources[0].1);

    // One changed byte in the fourth chunk.
    let mut damaged = original.clone();
    damaged[at + 100] ^= 0x01;
    let damaged_path = dir.join("damaged.mbundle");
    std::fs::write(&damaged_path, &damaged).unwrap();
    let out = dir.join("out");
    assert_eq!(
        import_streaming(&damaged_path, &vk, Some(&out)),
        Err(BundleError::ChunkDigestMismatch { index: 0, chunk: 3 })
    );
    assert!(!out.exists(), "a refused import created its directory");
    assert!(!dir.join(".out.partial").exists(), "staging left behind");
    assert!(matches!(
        read_bundle(&damaged_path),
        Err(BundleError::ChunkDigestMismatch { index: 0, chunk: 3 })
    ));

    // The same change with the chunk's digest rewritten to match: the chunk check
    // passes, and the signed payload digest refuses it.
    let digest = sha256(&damaged[at..at + CHUNK as usize]);
    damaged[at - 32..at].copy_from_slice(&digest);
    std::fs::write(&damaged_path, &damaged).unwrap();
    assert_eq!(
        import_streaming(&damaged_path, &vk, Some(&out)),
        Err(BundleError::Artefact {
            index: 0,
            error: VerifyError::PayloadDigestMismatch
        })
    );
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_wrong_key_truncation_trailing_bytes_or_an_existing_file_is_refused() {
    let dir = scratch("refused");
    let (vk, sk) = keys();
    let (bundle, _) = packed(&dir, &sk);
    let original = std::fs::read(&bundle).unwrap();
    let out = dir.join("out");

    let (other, _) = keys();
    assert_eq!(
        import_streaming(&bundle, &other, Some(&out)),
        Err(BundleError::BadBundleSignature)
    );
    assert!(!out.exists());

    let truncated = dir.join("truncated.mbundle");
    std::fs::write(&truncated, &original[..original.len() - 1000]).unwrap();
    assert!(matches!(
        import_streaming(&truncated, &vk, Some(&out)),
        Err(BundleError::Io(_))
    ));
    assert!(!out.exists());

    let trailing = dir.join("trailing.mbundle");
    let mut longer = original.clone();
    longer.push(0);
    std::fs::write(&trailing, longer).unwrap();
    assert!(matches!(
        import_streaming(&trailing, &vk, Some(&out)),
        Err(BundleError::Io(m)) if m.contains("trailing")
    ));
    assert!(!out.exists());

    // An import never overwrites, and refuses before writing anything.
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("model.manifest"), b"already here").unwrap();
    assert!(matches!(
        import_streaming(&bundle, &vk, Some(&out)),
        Err(BundleError::Io(m)) if m.contains("never overwrites")
    ));
    assert!(!out.join("granite.gguf").exists());
    assert_eq!(
        std::fs::read(out.join("model.manifest")).unwrap(),
        b"already here"
    );

    assert!(pack_streaming(dir.join("zero.mbundle"), "b", "l", &[], &sk, 0).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_version_1_bundle_imports_through_the_streaming_path() {
    let dir = scratch("v1");
    let (vk, sk) = keys();
    let sbom = generate_sbom(
        "heimdall",
        "0.9.0",
        "service",
        vec![SbomComponent::new("heimdall", "0.9.0")],
    );
    let artefact = build_and_sign(
        "heimdall",
        "0.9.0",
        "service",
        b"a small service".to_vec(),
        sbom,
        &sk,
    );
    let bundle = dir.join("v1.mgbundle");
    write_bundle(
        &bundle,
        &build_bundle("release", "stable", vec![artefact], &sk),
    )
    .unwrap();

    let out = dir.join("out");
    let refs = import_streaming(&bundle, &vk, Some(&out)).unwrap();
    assert_eq!(refs[0].id, "heimdall");
    assert_eq!(
        std::fs::read(out.join("heimdall")).unwrap(),
        b"a small service"
    );
    let (other, _) = keys();
    assert_eq!(
        import_streaming(&bundle, &other, Some(&dir.join("other"))),
        Err(BundleError::BadBundleSignature)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
