//! A bundle file is untrusted until its signature is checked, so a hostile entry or
//! artefact count in an inert 26-byte header must be refused as a truncated bundle,
//! not sized into an allocation that aborts the importer; and an import of it leaves
//! nothing behind. [VLD-ANB-MIDGARD-0044] [AC-35.3] [FR-B-5] [AC-B-3]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use bifrost_bundle::{import_streaming, read_bundle, BundleError};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::{OsRng, SignatureScheme, VerifyingKey};

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir =
        std::env::temp_dir().join(format!("bundle-counts-{tag}-{}-{uniq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn trust_key() -> VerifyingKey {
    FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
        .keypair(&mut OsRng)
        .unwrap()
        .0
}

/// The magic, version 1, signature algorithm 2 with an empty signature, an empty bundle
/// id and label, then `counts` as little-endian u32s.
fn header(counts: &[u32]) -> Vec<u8> {
    let mut b = b"MGBUNDLE".to_vec();
    b.push(1);
    b.push(2);
    for field in [0u32, 0, 0].iter().chain(counts) {
        b.extend_from_slice(&field.to_le_bytes());
    }
    b
}

fn is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().next().is_none()
}

/// `read_bundle` refuses the file as truncated, and `import_streaming` refuses it too,
/// writing nothing into the output directory.
fn assert_refused(tag: &str, bytes: &[u8]) {
    let dir = scratch(tag);
    let path = dir.join("hostile.mgb");
    std::fs::write(&path, bytes).unwrap();
    match read_bundle(&path) {
        Err(BundleError::Io(m)) => assert!(m.contains("truncated bundle"), "{m}"),
        other => panic!("a hostile count was not refused: {:?}", other.map(|_| ())),
    }
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    assert!(import_streaming(&path, &trust_key(), Some(&out)).is_err());
    assert!(is_empty(&out));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_hostile_entry_count_is_refused_not_allocated() {
    assert_refused("entries", &header(&[u32::MAX]));
}

#[test]
fn a_hostile_artefact_count_is_refused() {
    assert_refused("artefacts", &header(&[0, u32::MAX]));
}
