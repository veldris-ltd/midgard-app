//! Pipeline: dual-builder equivalence (FR-F-1), predicate-pushdown that preserves
//! results, deterministic end-to-end builds (AC-F-2), and scheduling (FR-F-5).
#![cfg(feature = "backend-fips")]

use forge_dataset_store::{Column, ColumnType, DataValue, Dataset, DatasetStore, Lineage, Schema};
use forge_engine::{CmpOp, ComputeBackend, ExecutionPlan, Predicate, ReferenceBackend, Transform};
use forge_pipeline::{codegen, optimise, run, GraphBuilder, Pipeline, Schedule, Scheduler};
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

fn store(seed: u64) -> DatasetStore<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "forge", &mut SeedRng(seed)).unwrap();
    DatasetStore::new(Kms::new(provider, root))
}

fn keep_big() -> Predicate {
    Predicate::Compare {
        column: "units".into(),
        op: CmpOp::Ge,
        value: DataValue::Int(100),
    }
}

#[test]
fn low_code_and_pro_code_produce_identical_ir() {
    // FR-F-1: the visual builder and the code path yield the SAME pipeline.
    let low = GraphBuilder::new("rev", "raw")
        .filter(keep_big())
        .rename("units", "qty")
        .build();

    let pro = Pipeline::code(
        "rev",
        "raw",
        vec![
            Transform::Filter(keep_big()),
            Transform::Rename {
                from: "units".into(),
                to: "qty".into(),
            },
        ],
    );

    assert_eq!(low, pro);
}

#[test]
fn predicate_pushdown_reorders_but_preserves_results() {
    // A filter authored after a rename it doesn't touch is pushed ahead of it.
    let p = Pipeline::code(
        "p",
        "raw",
        vec![
            Transform::Rename {
                from: "region".into(),
                to: "market".into(),
            },
            Transform::Filter(keep_big()), // references 'units', untouched by the rename
        ],
    );
    let opt = optimise(&p);
    assert!(
        opt.stages[0].is_filter(),
        "filter should be pushed to the front"
    );

    // Same output as the unoptimised pipeline.
    let backend = ReferenceBackend::new();
    let input = raw();
    let a = backend
        .execute(&ExecutionPlan::new(p.stages.clone()), &input)
        .unwrap();
    let b = backend.execute(&codegen(&opt), &input).unwrap();
    assert_eq!(a, b);
    assert_eq!(b.row_count(), 2);
}

#[test]
fn pushdown_blocked_by_referenced_rename_and_by_limit() {
    // Filter references the RENAMED column → cannot precede the rename.
    let p1 = Pipeline::code(
        "p1",
        "raw",
        vec![
            Transform::Rename {
                from: "units".into(),
                to: "qty".into(),
            },
            Transform::Filter(Predicate::Compare {
                column: "qty".into(),
                op: CmpOp::Ge,
                value: DataValue::Int(100),
            }),
        ],
    );
    let o1 = optimise(&p1);
    assert!(
        !o1.stages[0].is_filter(),
        "rename must stay ahead of the filter it feeds"
    );

    // Filter after a Limit must not cross it (would change the row set).
    let p2 = Pipeline::code(
        "p2",
        "raw",
        vec![Transform::Limit(1), Transform::Filter(keep_big())],
    );
    let o2 = optimise(&p2);
    assert_eq!(o2.stages, p2.stages);
}

#[test]
fn end_to_end_build_is_reproducible_across_independent_stores() {
    // AC-F-2: build the same pipeline over the same input in two independent stores;
    // the committed output content ids match.
    let pipe = GraphBuilder::new("curate", "raw")
        .filter(keep_big())
        .select(vec!["region".into(), "units".into()])
        .build();
    let backend = ReferenceBackend::new();

    let mut s1 = store(1);
    s1.commit(
        &raw(),
        "raw",
        Lineage::ingest("connector:file:sales.csv"),
        &mut SeedRng(10),
    )
    .unwrap();
    let out1 = run(&pipe, &mut s1, &backend, "curated", &mut SeedRng(11)).unwrap();

    let mut s2 = store(2);
    s2.commit(
        &raw(),
        "raw",
        Lineage::ingest("connector:file:sales.csv"),
        &mut SeedRng(77),
    )
    .unwrap();
    let out2 = run(&pipe, &mut s2, &backend, "curated", &mut SeedRng(88)).unwrap();

    assert_eq!(out1.version.content_id, out2.version.content_id);
    assert_eq!(out1.version.row_count, 2);
    assert_eq!(out1.backend, "reference");
    // Output lineage records the pipeline and its input.
    assert_eq!(out1.version.lineage.source, "pipeline:curate");
    assert_eq!(out1.version.parents, vec![out1.input_id]);
}

#[test]
fn build_fails_closed_on_type_error_and_unknown_source() {
    let backend = ReferenceBackend::new();
    let mut s = store(3);
    s.commit(&raw(), "raw", Lineage::ingest("x"), &mut SeedRng(1))
        .unwrap();

    // Selecting a non-existent column is a type error → nothing committed.
    let bad = Pipeline::code("bad", "raw", vec![Transform::Select(vec!["nope".into()])]);
    assert!(run(&bad, &mut s, &backend, "out", &mut SeedRng(2)).is_err());
    assert!(s.branch_head("out").is_none());

    // Unknown source branch fails closed.
    let orphan = Pipeline::code("o", "missing", vec![]);
    assert!(run(&orphan, &mut s, &backend, "out", &mut SeedRng(3)).is_err());
}

#[test]
fn scheduler_separates_batch_stream_and_manual() {
    let mut sched = Scheduler::new();
    sched.register(Schedule::batch("nightly-rollup"));
    sched.register(Schedule::stream("live-events"));
    sched.register(Schedule::manual("adhoc"));

    assert_eq!(sched.due_on_tick(), vec!["nightly-rollup"]);
    assert_eq!(sched.due_on_event(), vec!["live-events"]);
    assert_eq!(sched.manual(), vec!["adhoc"]);
}
