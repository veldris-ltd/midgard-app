//! The partitioned, multi-worker backend must match the single-threaded reference
//! oracle byte-for-byte — including through a real hash shuffle (aggregate) and a
//! forced spill-to-disk. [FR-F-1] [AC-F-2]

use forge_dataset_store::{Column, ColumnType, DataValue, Dataset, Schema};
use forge_engine::{
    AggFunc, AggregateSpec, ComputeBackend, ExecutionPlan, JoinType, PartitionedBackend, Predicate,
    ReferenceBackend, SortKey, Transform,
};

/// A wider sales dataset so partitioning + shuffle actually spread work.
fn sales(n: usize) -> Dataset {
    let regions = ["EMEA", "APAC", "NORAM", "LATAM"];
    let mut rows = Vec::new();
    for i in 0..n {
        rows.push(vec![
            DataValue::Text(regions[i % regions.len()].into()),
            DataValue::Text(format!("rep-{}", i % 7)),
            DataValue::Int((i as i64 * 13 % 500) + 1),
        ]);
    }
    Dataset::new(
        Schema::new(vec![
            Column::new("region", ColumnType::Text),
            Column::new("rep", ColumnType::Text),
            Column::new("amount", ColumnType::Int),
        ]),
        rows,
    )
    .unwrap()
}

fn dim() -> Dataset {
    Dataset::new(
        Schema::new(vec![
            Column::new("region", ColumnType::Text),
            Column::new("manager", ColumnType::Text),
        ]),
        vec![
            vec![
                DataValue::Text("EMEA".into()),
                DataValue::Text("Mara".into()),
            ],
            vec![
                DataValue::Text("APAC".into()),
                DataValue::Text("Ola".into()),
            ],
            vec![
                DataValue::Text("NORAM".into()),
                DataValue::Text("Ken".into()),
            ],
            vec![
                DataValue::Text("LATAM".into()),
                DataValue::Text("Sol".into()),
            ],
        ],
    )
    .unwrap()
}

/// filter → join → aggregate → sort: the RC-27 shape, plus a sort to pin order.
fn pipeline() -> ExecutionPlan {
    ExecutionPlan::new(vec![
        Transform::Filter(Predicate::Compare {
            column: "amount".into(),
            op: forge_engine::CmpOp::Gt,
            value: DataValue::Int(50),
        }),
        Transform::Join {
            right: dim(),
            left_on: vec!["region".into()],
            right_on: vec!["region".into()],
            how: JoinType::Inner,
        },
        Transform::Aggregate {
            group_by: vec!["manager".into()],
            aggs: vec![
                AggregateSpec {
                    func: AggFunc::Sum("amount".into()),
                    output: "total".into(),
                },
                AggregateSpec {
                    func: AggFunc::Count,
                    output: "n".into(),
                },
            ],
        },
        Transform::Sort(vec![SortKey::desc("total")]),
    ])
}

#[test]
fn partitioned_matches_reference_with_shuffle_and_spill() {
    let input = sales(400);
    let plan = pipeline();

    let reference = ReferenceBackend::new().execute(&plan, &input).unwrap();

    // 4 workers, spill any partition over 30 rows (400 rows / 4 = 100 > 30 → spills).
    let spill_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| ".".into());
    let partitioned = PartitionedBackend::new(4)
        .with_spill(30, spill_dir)
        .execute(&plan, &input)
        .unwrap();

    // Byte-identical: same schema, same rows, same order.
    assert_eq!(reference.schema(), partitioned.schema());
    assert_eq!(reference.rows(), partitioned.rows());
    // And it actually computed something (4 managers).
    assert_eq!(reference.row_count(), 4);
}

#[test]
fn partitioned_matches_reference_across_worker_counts() {
    let input = sales(233); // deliberately not divisible by the worker counts
    let plan = pipeline();
    let reference = ReferenceBackend::new().execute(&plan, &input).unwrap();
    for workers in [1usize, 2, 3, 8, 16] {
        let out = PartitionedBackend::new(workers)
            .execute(&plan, &input)
            .unwrap();
        assert_eq!(
            reference.rows(),
            out.rows(),
            "worker count {workers} diverged from the reference"
        );
    }
}
