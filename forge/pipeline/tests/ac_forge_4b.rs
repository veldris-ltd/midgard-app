//! Phase-4.b acceptance — the consolidated Given/When/Then checks for the ingest →
//! version → build stack. Exercises the full path with **no Mimir dependency in the
//! graph** (AC-F-6) and runs entirely offline (AC-F-7 when executed under
//! `docker --network none`).
//!
//! - AC-F-1  connector sync → PQ-sealed raw dataset. [FR-F-2] [NFR-PQ-2]
//! - AC-F-2  a pipeline built twice from the same inputs is content-identical. [FR-F-3]
//! - AC-F-6  all non-AI pipeline features work with Mimir absent (standalone). [DD-6]
//! - AC-F-7  all core data operations succeed with no outbound connectivity. [NFR-PRV-1]
#![cfg(feature = "backend-fips")]

use forge_connector_sdk::connectors::file::DelimitedConnector;
use forge_connector_sdk::{sync_to_store, NoCredentials, SandboxPolicy, SyncContext};
use forge_dataset_store::{Column, ColumnType, DataValue, DatasetStore, Schema};
use forge_engine::{CmpOp, Predicate, ReferenceBackend};
use forge_pipeline::{run, GraphBuilder};
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

fn sales_schema() -> Schema {
    Schema::new(vec![
        Column::new("region", ColumnType::Text),
        Column::new("units", ColumnType::Int),
        Column::nullable("note", ColumnType::Text),
    ])
}

const SALES_CSV: &str = "region,units,note\nemea,120,\namer,340,q4 spike\napac,90,\n";

fn store(seed: u64) -> DatasetStore<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "forge-ac", &mut SeedRng(seed)).unwrap();
    DatasetStore::new(Kms::new(provider, root))
}

/// AC-F-1: syncing a source produces a raw dataset encrypted with a PQ-wrapped key.
#[test]
fn ac_f_1_connector_sync_creates_a_pq_sealed_raw_dataset() {
    let mut s = store(1);
    let conn = DelimitedConnector::csv("sales.csv", sales_schema(), SALES_CSV);
    let sandbox = SandboxPolicy::air_gapped();
    let ctx = SyncContext::new(&NoCredentials, &sandbox);

    let version = sync_to_store(&conn, &ctx, &mut s, "raw", &mut SeedRng(2)).unwrap();

    let stored = s.get(&version.content_id).unwrap();
    // Sealed under ML-KEM-1024 to the trust root, no plaintext in the ciphertext.
    assert_eq!(stored.sealed.suite, pqc_agility::KemSuiteId::MlKem1024);
    assert_eq!(stored.sealed.root_key_id, "forge-ac");
    assert!(!stored
        .sealed
        .ciphertext
        .windows(8)
        .any(|w| w == b"q4 spike"));
    // And it is a real, recoverable raw dataset.
    assert_eq!(s.checkout(&version.content_id).unwrap().row_count(), 3);
}

/// AC-F-2: a pipeline built twice from the same inputs yields content-identical
/// output versions.
#[test]
fn ac_f_2_pipeline_rebuild_is_content_identical() {
    let conn = DelimitedConnector::csv("sales.csv", sales_schema(), SALES_CSV);
    let sandbox = SandboxPolicy::air_gapped();
    let ctx = SyncContext::new(&NoCredentials, &sandbox);
    let backend = ReferenceBackend::new();
    let pipe = GraphBuilder::new("curate", "raw")
        .filter(Predicate::Compare {
            column: "units".into(),
            op: CmpOp::Ge,
            value: DataValue::Int(100),
        })
        .select(vec!["region".into(), "units".into()])
        .build();

    // Build A.
    let mut a = store(10);
    sync_to_store(&conn, &ctx, &mut a, "raw", &mut SeedRng(1)).unwrap();
    let ra = run(&pipe, &mut a, &backend, "curated", &mut SeedRng(2)).unwrap();

    // Build B in a completely independent store.
    let mut b = store(20);
    sync_to_store(&conn, &ctx, &mut b, "raw", &mut SeedRng(3)).unwrap();
    let rb = run(&pipe, &mut b, &backend, "curated", &mut SeedRng(4)).unwrap();

    assert_eq!(ra.version.content_id, rb.version.content_id);
    assert_eq!(ra.version.row_count, 2); // apac (90) filtered out
}

/// AC-F-6 / AC-F-7: the entire ingest→version→build→checkout flow runs to
/// completion with no Mimir crate in the dependency graph and (when run under
/// `--network none`) no outbound connectivity. This test *is* that flow.
#[test]
fn ac_f_6_and_7_full_flow_standalone_and_offline() {
    let mut s = store(30);
    let conn = DelimitedConnector::csv("sales.csv", sales_schema(), SALES_CSV);
    let sandbox = SandboxPolicy::air_gapped(); // denies all egress by default
    let ctx = SyncContext::new(&NoCredentials, &sandbox);
    let backend = ReferenceBackend::new();

    // Ingest.
    sync_to_store(&conn, &ctx, &mut s, "raw", &mut SeedRng(1)).unwrap();
    // Author (low-code) + build.
    let pipe = GraphBuilder::new("curate", "raw")
        .filter(Predicate::Compare {
            column: "units".into(),
            op: CmpOp::Ge,
            value: DataValue::Int(100),
        })
        .rename("units", "qty")
        .build();
    let outcome = run(&pipe, &mut s, &backend, "curated", &mut SeedRng(2)).unwrap();

    // Curated result is recoverable and correctly shaped.
    let curated = s.checkout_branch("curated").unwrap();
    assert_eq!(curated.row_count(), 2);
    assert_eq!(curated.schema().column("qty").unwrap().ty, ColumnType::Int);
    assert!(curated.schema().column("units").is_none()); // renamed
    assert_eq!(outcome.backend, "reference");
}
