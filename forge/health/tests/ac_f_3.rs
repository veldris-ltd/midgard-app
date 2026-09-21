//! AC-F-3 — Given failing health expectations, when a build runs, then promotion is
//! blocked and the failure is reported. [FR-F-6]
#![cfg(feature = "backend-fips")]

use forge_dataset_store::{Column, ColumnType, DataValue, Dataset, DatasetStore, Lineage, Schema};
use forge_engine::{CmpOp, Predicate, ReferenceBackend};
use forge_health::{run_gated, Expectation, HealthSuite};
use forge_pipeline::{GraphBuilder, Pipeline};
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

fn schema() -> Schema {
    Schema::new(vec![
        Column::new("region", ColumnType::Text),
        Column::new("units", ColumnType::Int),
    ])
}

fn raw() -> Dataset {
    Dataset::new(
        schema(),
        vec![
            vec![DataValue::Text("emea".into()), DataValue::Int(120)],
            vec![DataValue::Text("amer".into()), DataValue::Int(340)],
            vec![DataValue::Text("apac".into()), DataValue::Int(90)],
        ],
    )
    .unwrap()
}

fn store_with_raw(seed: u64) -> DatasetStore<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "forge", &mut SeedRng(seed)).unwrap();
    let mut s = DatasetStore::new(Kms::new(provider, root));
    s.commit(
        &raw(),
        "raw",
        Lineage::ingest("connector:file:sales.csv"),
        &mut SeedRng(seed + 1),
    )
    .unwrap();
    s
}

fn identity_pipeline() -> Pipeline {
    GraphBuilder::new("passthrough", "raw")
        .select(vec!["region".into(), "units".into()])
        .build()
}

#[test]
fn failing_expectation_blocks_promotion_and_reports() {
    // Expect units in [100, 1000] — apac (90) violates it.
    let mut s = store_with_raw(1);
    let suite = HealthSuite::new(vec![Expectation::IntInRange {
        column: "units".into(),
        min: 100,
        max: 1000,
    }]);
    let backend = ReferenceBackend::new();

    let outcome = run_gated(
        &identity_pipeline(),
        &mut s,
        &backend,
        &suite,
        "curated",
        &mut SeedRng(9),
    )
    .unwrap();

    // Promotion is BLOCKED and the failure is reported.
    assert!(!outcome.report.passed);
    assert!(!outcome.was_promoted());
    assert_eq!(outcome.promoted, None);
    assert!(
        s.branch_head("curated").is_none(),
        "nothing committed on failure"
    );
    let failures = outcome.report.failures();
    assert_eq!(failures.len(), 1);
    assert!(failures[0].detail.contains("outside [100, 1000]"));
}

#[test]
fn passing_expectations_allow_promotion() {
    // Filter to units >= 100 first, then the same expectation passes → promotes.
    let mut s = store_with_raw(2);
    let pipe = GraphBuilder::new("curate", "raw")
        .filter(Predicate::Compare {
            column: "units".into(),
            op: CmpOp::Ge,
            value: DataValue::Int(100),
        })
        .build();
    let suite = HealthSuite::new(vec![
        Expectation::IntInRange {
            column: "units".into(),
            min: 100,
            max: 1000,
        },
        Expectation::NotNull {
            column: "region".into(),
        },
        Expectation::RowCountAtLeast(1),
    ]);
    let backend = ReferenceBackend::new();

    let outcome = run_gated(&pipe, &mut s, &backend, &suite, "curated", &mut SeedRng(9)).unwrap();

    assert!(outcome.report.passed, "{}", outcome.report.summary());
    assert!(outcome.was_promoted());
    let version = outcome.promoted.unwrap();
    assert_eq!(version.row_count, 2);
    // The promoted version is recoverable at the branch head.
    assert_eq!(s.branch_head("curated"), Some(version.content_id));
}

#[test]
fn missing_column_expectation_fails_closed() {
    let mut s = store_with_raw(3);
    let suite = HealthSuite::new(vec![Expectation::NotNull {
        column: "nonexistent".into(),
    }]);
    let backend = ReferenceBackend::new();
    let outcome = run_gated(
        &identity_pipeline(),
        &mut s,
        &backend,
        &suite,
        "curated",
        &mut SeedRng(9),
    )
    .unwrap();
    assert!(!outcome.was_promoted());
    assert!(outcome.report.failures()[0]
        .detail
        .contains("does not exist"));
}
