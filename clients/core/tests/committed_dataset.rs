//! The dataset committed for this release verifies under the pinned demo key and is
//! exactly what its committed fixtures build. [NFR-PQ-3] [AC-C-13]

use std::fs;
use std::path::PathBuf;

use attest::TrustAnchors;
use midgard_client_core::demo::{build, dataset};
use midgard_client_core::CORE_VERSION;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_committed_dataset_verifies_and_matches_its_fixtures() {
    let path = repo().join(format!("clients/demo/datasets/{CORE_VERSION}.mgdemo"));
    let file = fs::read(&path).unwrap_or_else(|_| {
        panic!("no demo dataset for {CORE_VERSION}: run `just demo-dataset` (Task A7)")
    });
    let anchors = TrustAnchors::parse(
        &fs::read_to_string(repo().join("clients/demo/trust/demo-dataset.pub")).unwrap(),
    )
    .unwrap();
    let ds = dataset::verify(&file, &anchors, CORE_VERSION).unwrap();
    let body = build::canonical_body(&repo().join(format!("clients/demo/fixtures/{CORE_VERSION}")))
        .unwrap();
    assert_eq!(
        ds.body_bytes, body,
        "the dataset is stale: run `just demo-dataset`"
    );
}

#[test]
fn the_pinned_anchor_is_one_ml_dsa_key_for_demo_datasets_only() {
    let text = fs::read_to_string(repo().join("clients/demo/trust/demo-dataset.pub")).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "exactly one demo anchor: {lines:?}");
    let fields: Vec<&str> = lines[0].split(' ').collect();
    assert_eq!(fields.len(), 3, "<alg> <key-id> <hex>");
    assert!(
        dataset::APPROVED.iter().any(|a| a.as_str() == fields[0]),
        "{} is not a demo-dataset algorithm",
        fields[0]
    );
    assert!(
        fields[1].starts_with("midgard-demo-dataset-"),
        "{}",
        fields[1]
    );
    // A public key only: hex, nothing that looks like keystore or PEM material.
    assert!(fields[2].bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(!text.contains("PRIVATE"));
}
