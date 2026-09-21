//! RC-11: the dataset/version store is durable. Given committed pipeline outputs,
//! when the store restarts, `checkout_branch` returns the identical dataset and
//! lineage; version blobs are encrypted at rest; a tampered blob fails closed.
//! [SAD_01 §13] [RC-11]
#![cfg(feature = "backend-fips")]

use std::path::PathBuf;

use forge_dataset_store::{
    Column, ColumnType, DataValue, Dataset, DurableDatasetStore, Lineage, Schema,
};
use kms::{Kms, SoftwareTrustRoot};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

const ROOT_SEED: u64 = 1357;

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let b = z.to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&b[..take]);
            i += take;
        }
    }
}

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "forge-durable-{tag}-{}-{}",
        std::process::id(),
        uniq
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Same KMS root regenerated (same seed) => same identity across a restart.
fn open(dir: &PathBuf) -> DurableDatasetStore<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root =
        SoftwareTrustRoot::generate(&provider, "forge-root", &mut SeedRng(ROOT_SEED)).unwrap();
    DurableDatasetStore::open(dir, Kms::new(provider, root)).unwrap()
}

fn schema() -> Schema {
    Schema::new(vec![
        Column::new("region", ColumnType::Text),
        Column::new("units", ColumnType::Int),
        Column::nullable("note", ColumnType::Text),
    ])
}

fn raw() -> Dataset {
    Dataset::new(
        schema(),
        vec![
            vec![
                DataValue::Text("emea".into()),
                DataValue::Int(120),
                DataValue::Null,
            ],
            vec![
                DataValue::Text("amer".into()),
                DataValue::Int(340),
                DataValue::Text("q4 spike".into()),
            ],
            vec![
                DataValue::Text("apac".into()),
                DataValue::Int(90),
                DataValue::Null,
            ],
        ],
    )
    .unwrap()
}

/// A curated transform output (region=amer only) — a distinct content id.
fn curated() -> Dataset {
    Dataset::new(
        schema(),
        vec![vec![
            DataValue::Text("amer".into()),
            DataValue::Int(340),
            DataValue::Text("q4 spike".into()),
        ]],
    )
    .unwrap()
}

#[test]
fn checkout_branch_is_identical_after_restart() {
    let dir = scratch("ok");

    // --- session 1: a two-hop pipeline (ingest -> transform), two branches ---
    let (raw_id, curated_id) = {
        let mut store = open(&dir);
        let v_raw = store
            .commit(
                &raw(),
                "raw",
                Lineage::ingest("connector:file:sales.csv"),
                &mut SeedRng(1),
            )
            .unwrap();
        let v_cur = store
            .commit(
                &curated(),
                "main",
                Lineage::transform(
                    "pipeline:na-sales",
                    "transform:filter",
                    vec![v_raw.content_id],
                ),
                &mut SeedRng(2),
            )
            .unwrap();
        assert_eq!(store.len(), 2);
        (v_raw.content_id, v_cur.content_id)
    }; // store dropped: simulated restart.

    // Encrypted at rest: no plaintext cell value appears in any version blob.
    for entry in std::fs::read_dir(dir.join("objects")).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for needle in [b"emea".as_slice(), b"q4 spike", b"apac"] {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle),
                "plaintext {:?} must not be in a sealed blob",
                String::from_utf8_lossy(needle)
            );
        }
    }

    // --- restart: reopen and check out both branches, byte-for-byte ---
    let store = open(&dir);
    assert_eq!(store.len(), 2, "all versions recovered");
    assert_eq!(store.branch_head("main"), Some(curated_id));
    assert_eq!(store.branch_head("raw"), Some(raw_id));

    assert_eq!(
        store.checkout_branch("main").unwrap(),
        curated(),
        "main head identical after restart"
    );
    assert_eq!(
        store.checkout_branch("raw").unwrap(),
        raw(),
        "raw head identical after restart"
    );

    // Lineage recovered: the curated version records the raw version as its input.
    let cur = store.get(&curated_id).unwrap();
    assert_eq!(cur.version.lineage.operation, "transform:filter");
    assert_eq!(cur.version.lineage.inputs, vec![raw_id]);
    assert_eq!(cur.version.parents, vec![raw_id]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tampered_blob_fails_closed_on_checkout() {
    let dir = scratch("tamper");
    let cid = {
        let mut store = open(&dir);
        let v = store
            .commit(
                &raw(),
                "main",
                Lineage::ingest("connector:file:sales.csv"),
                &mut SeedRng(1),
            )
            .unwrap();
        v.content_id
    };

    // Flip a byte inside the sealed version blob (its filename/header content id is
    // unchanged, but the AEAD ciphertext is now corrupt).
    let blob = dir.join("objects").join(cid.to_hex());
    let mut bytes = std::fs::read(&blob).unwrap();
    let last = bytes.len() - 4; // inside the AES-GCM ciphertext/tag region
    bytes[last] ^= 0x01;
    std::fs::write(&blob, &bytes).unwrap();

    // Reopen succeeds (header still parses), but checkout unseals and fails closed.
    let store = open(&dir);
    assert!(
        store.checkout_branch("main").is_err(),
        "a tampered blob must not return data"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
