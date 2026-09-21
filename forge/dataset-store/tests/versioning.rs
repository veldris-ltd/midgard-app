//! Dataset-store foundations: content-addressed reproducibility (AC-F-2), sealing
//! at rest (AC-F-1), round-trip recovery, and lineage. [FR-F-3] [NFR-PQ-2]
#![cfg(feature = "backend-fips")]

use forge_dataset_store::{Column, ColumnType, DataValue, Dataset, DatasetStore, Lineage, Schema};
use kms::{Kms, SoftwareTrustRoot};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

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

fn store(seed: u64, key_id: &str) -> DatasetStore<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, key_id, &mut SeedRng(seed)).unwrap();
    DatasetStore::new(Kms::new(provider, root))
}

fn sales_schema() -> Schema {
    Schema::new(vec![
        Column::new("region", ColumnType::Text),
        Column::new("units", ColumnType::Int),
        Column::nullable("note", ColumnType::Text),
    ])
}

fn sales_rows() -> Vec<Vec<DataValue>> {
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
    ]
}

#[test]
fn same_inputs_produce_identical_content_id_across_independent_stores() {
    // AC-F-2: two independent stores (different roots, different seal nonces) commit
    // the same data to the SAME content id — reproducibility is over plaintext bytes,
    // not the encryption envelope.
    let ds = Dataset::new(sales_schema(), sales_rows()).unwrap();

    let mut a = store(1, "root-a");
    let mut b = store(2, "root-b");
    let va = a
        .commit(
            &ds,
            "main",
            Lineage::ingest("connector:file:sales.csv"),
            &mut SeedRng(10),
        )
        .unwrap();
    let vb = b
        .commit(
            &ds,
            "main",
            Lineage::ingest("connector:file:sales.csv"),
            &mut SeedRng(99),
        )
        .unwrap();

    assert_eq!(va.content_id, vb.content_id, "content ids must match");
    assert_eq!(va.schema_hash, vb.schema_hash);
    assert_eq!(va.row_count, 2);

    // The sealed blobs differ (fresh nonces / different roots), proving the content
    // id does not leak from the ciphertext.
    let sa = a.get(&va.content_id).unwrap();
    let sb = b.get(&vb.content_id).unwrap();
    assert_ne!(sa.sealed.ciphertext, sb.sealed.ciphertext);
}

#[test]
fn snapshot_is_sealed_at_rest_and_carries_no_plaintext() {
    // AC-F-1: the committed snapshot is encrypted under a PQ-wrapped key.
    let ds = Dataset::new(sales_schema(), sales_rows()).unwrap();
    let mut s = store(3, "root-c");
    let v = s
        .commit(
            &ds,
            "main",
            Lineage::ingest("connector:file:sales.csv"),
            &mut SeedRng(7),
        )
        .unwrap();

    let stored = s.get(&v.content_id).unwrap();
    // ML-KEM-1024 wrap suite, sealed to the named root.
    assert_eq!(stored.sealed.suite, pqc_agility::KemSuiteId::MlKem1024);
    assert_eq!(stored.sealed.root_key_id, "root-c");
    // Plaintext string does not appear in the ciphertext.
    let needle = b"q4 spike";
    assert!(!stored
        .sealed
        .ciphertext
        .windows(needle.len())
        .any(|w| w == needle));
}

#[test]
fn checkout_roundtrips_through_the_trust_root() {
    let ds = Dataset::new(sales_schema(), sales_rows()).unwrap();
    let mut s = store(4, "root-d");
    let v = s
        .commit(
            &ds,
            "main",
            Lineage::ingest("connector:file:sales.csv"),
            &mut SeedRng(5),
        )
        .unwrap();

    let recovered = s.checkout(&v.content_id).unwrap();
    assert_eq!(recovered, ds);
    // And by branch head.
    assert_eq!(s.checkout_branch("main").unwrap(), ds);
    assert_eq!(s.branch_head("main"), Some(v.content_id));
}

#[test]
fn lineage_records_source_operation_and_inputs() {
    let ds = Dataset::new(sales_schema(), sales_rows()).unwrap();
    let mut s = store(6, "root-f");
    let raw = s
        .commit(
            &ds,
            "raw",
            Lineage::ingest("connector:file:sales.csv"),
            &mut SeedRng(1),
        )
        .unwrap();

    // A derived commit records its parent.
    let curated = Dataset::new(
        sales_schema(),
        vec![sales_rows()[1].clone()], // "curated" subset
    )
    .unwrap();
    let derived = s
        .commit(
            &curated,
            "curated",
            Lineage::transform(
                "pipeline:filter-amer",
                "transform:filter",
                vec![raw.content_id],
            ),
            &mut SeedRng(2),
        )
        .unwrap();

    assert_eq!(derived.parents, vec![raw.content_id]);
    assert_eq!(derived.lineage.operation, "transform:filter");
    assert_eq!(derived.lineage.inputs, vec![raw.content_id]);
    assert_eq!(s.len(), 2);
}

#[test]
fn invalid_rows_are_rejected_before_the_store() {
    // Arity mismatch fails closed at construction.
    let bad = Dataset::new(
        sales_schema(),
        vec![vec![DataValue::Text("x".into()), DataValue::Int(1)]], // 2 cells, 3 columns
    );
    assert!(bad.is_err());

    // Type mismatch fails closed.
    let bad2 = Dataset::new(
        sales_schema(),
        vec![vec![
            DataValue::Int(1), // region should be text
            DataValue::Int(1),
            DataValue::Null,
        ]],
    );
    assert!(bad2.is_err());
}
