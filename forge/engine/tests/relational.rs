//! Reference-executor coverage for the new relational IR ops: sort, aggregate,
//! join, window. The reference backend is the correctness oracle every other backend
//! must match. [FR-F-1]

use forge_dataset_store::{Column, ColumnType, DataValue, Dataset, Schema};
use forge_engine::{
    AggFunc, AggregateSpec, ComputeBackend, ExecutionPlan, JoinType, ReferenceBackend, SortKey,
    Transform, WindowFunc,
};

fn sales() -> Dataset {
    // region, rep, amount
    Dataset::new(
        Schema::new(vec![
            Column::new("region", ColumnType::Text),
            Column::new("rep", ColumnType::Text),
            Column::new("amount", ColumnType::Int),
        ]),
        vec![
            vec![
                DataValue::Text("EMEA".into()),
                DataValue::Text("ana".into()),
                DataValue::Int(100),
            ],
            vec![
                DataValue::Text("EMEA".into()),
                DataValue::Text("bo".into()),
                DataValue::Int(300),
            ],
            vec![
                DataValue::Text("APAC".into()),
                DataValue::Text("cy".into()),
                DataValue::Int(50),
            ],
            vec![
                DataValue::Text("EMEA".into()),
                DataValue::Text("ana".into()),
                DataValue::Int(200),
            ],
            vec![
                DataValue::Text("APAC".into()),
                DataValue::Text("cy".into()),
                DataValue::Int(70),
            ],
        ],
    )
    .unwrap()
}

fn run(plan: ExecutionPlan, input: &Dataset) -> Dataset {
    ReferenceBackend::new().execute(&plan, input).unwrap()
}

#[test]
fn aggregate_group_sum_count_avg_min_max() {
    let plan = ExecutionPlan::new(vec![Transform::Aggregate {
        group_by: vec!["region".into()],
        aggs: vec![
            AggregateSpec {
                func: AggFunc::Count,
                output: "n".into(),
            },
            AggregateSpec {
                func: AggFunc::Sum("amount".into()),
                output: "total".into(),
            },
            AggregateSpec {
                func: AggFunc::Avg("amount".into()),
                output: "mean".into(),
            },
            AggregateSpec {
                func: AggFunc::Min("amount".into()),
                output: "lo".into(),
            },
            AggregateSpec {
                func: AggFunc::Max("amount".into()),
                output: "hi".into(),
            },
        ],
    }]);
    let out = run(plan, &sales());
    // Groups emitted in key order: APAC, then EMEA.
    assert_eq!(out.row_count(), 2);
    assert_eq!(out.schema().width(), 6);
    let apac = &out.rows()[0];
    assert_eq!(apac[0], DataValue::Text("APAC".into()));
    assert_eq!(apac[1], DataValue::Int(2)); // count
    assert_eq!(apac[2], DataValue::Int(120)); // sum 50+70
    assert_eq!(apac[3], DataValue::Float(60.0)); // avg
    assert_eq!(apac[4], DataValue::Int(50)); // min
    assert_eq!(apac[5], DataValue::Int(70)); // max
    let emea = &out.rows()[1];
    assert_eq!(emea[1], DataValue::Int(3));
    assert_eq!(emea[2], DataValue::Int(600)); // 100+300+200
    assert_eq!(emea[4], DataValue::Int(100));
    assert_eq!(emea[5], DataValue::Int(300));
}

#[test]
fn sort_multi_key_desc_then_asc() {
    let plan = ExecutionPlan::new(vec![Transform::Sort(vec![
        SortKey::asc("region"),
        SortKey::desc("amount"),
    ])]);
    let out = run(plan, &sales());
    let regions: Vec<_> = out.rows().iter().map(|r| r[0].clone()).collect();
    let amounts: Vec<_> = out.rows().iter().map(|r| r[2].clone()).collect();
    // APAC block first (70,50), then EMEA block (300,200,100).
    assert_eq!(regions[0], DataValue::Text("APAC".into()));
    assert_eq!(amounts[0], DataValue::Int(70));
    assert_eq!(amounts[1], DataValue::Int(50));
    assert_eq!(regions[2], DataValue::Text("EMEA".into()));
    assert_eq!(amounts[2], DataValue::Int(300));
    assert_eq!(amounts[4], DataValue::Int(100));
}

fn regions_dim() -> Dataset {
    // region, manager  (EMEA + APAC; NORAM present but unused → tests inner drop)
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
        ],
    )
    .unwrap()
}

#[test]
fn inner_join_then_aggregate() {
    // Join sales → region managers, then total per manager. (RC-27 shape.)
    let plan = ExecutionPlan::new(vec![
        Transform::Join {
            right: regions_dim(),
            left_on: vec!["region".into()],
            right_on: vec!["region".into()],
            how: JoinType::Inner,
        },
        Transform::Aggregate {
            group_by: vec!["manager".into()],
            aggs: vec![AggregateSpec {
                func: AggFunc::Sum("amount".into()),
                output: "total".into(),
            }],
        },
    ]);
    let out = run(plan, &sales());
    // Manager order: Mara (EMEA 600), Ola (APAC 120).
    assert_eq!(out.row_count(), 2);
    assert_eq!(out.rows()[0][0], DataValue::Text("Mara".into()));
    assert_eq!(out.rows()[0][1], DataValue::Int(600));
    assert_eq!(out.rows()[1][0], DataValue::Text("Ola".into()));
    assert_eq!(out.rows()[1][1], DataValue::Int(120));
}

#[test]
fn left_join_keeps_unmatched_with_nulls() {
    // A left row whose region has no dimension match keeps null manager.
    let mut s = sales();
    let _ = &mut s;
    let dim = Dataset::new(
        Schema::new(vec![
            Column::new("region", ColumnType::Text),
            Column::new("manager", ColumnType::Text),
        ]),
        vec![vec![
            DataValue::Text("EMEA".into()),
            DataValue::Text("Mara".into()),
        ]],
    )
    .unwrap();
    let plan = ExecutionPlan::new(vec![Transform::Join {
        right: dim,
        left_on: vec!["region".into()],
        right_on: vec!["region".into()],
        how: JoinType::Left,
    }]);
    let out = run(plan, &sales());
    // Every left row survives; APAC rows have null manager.
    assert_eq!(out.row_count(), 5);
    let apac = out
        .rows()
        .iter()
        .find(|r| r[0] == DataValue::Text("APAC".into()))
        .unwrap();
    assert_eq!(apac[3], DataValue::Null); // manager column is null
                                          // The output manager column is nullable.
    assert!(out.schema().column("manager").unwrap().nullable);
}

#[test]
fn window_row_number_and_running_sum_per_partition() {
    let plan = ExecutionPlan::new(vec![
        Transform::Window {
            partition_by: vec!["region".into()],
            order_by: vec![SortKey::asc("amount")],
            func: WindowFunc::RowNumber,
            output: "rn".into(),
        },
        Transform::Window {
            partition_by: vec!["region".into()],
            order_by: vec![SortKey::asc("amount")],
            func: WindowFunc::RunningSum("amount".into()),
            output: "cum".into(),
        },
    ]);
    let out = run(plan, &sales());
    assert_eq!(out.row_count(), 5);
    assert_eq!(out.schema().width(), 5); // region,rep,amount,rn,cum
                                         // For EMEA amounts sorted asc: 100(rn1,cum100),200(rn2,cum300),300(rn3,cum600).
    let emea: Vec<_> = out
        .rows()
        .iter()
        .filter(|r| r[0] == DataValue::Text("EMEA".into()))
        .map(|r| (r[2].clone(), r[3].clone(), r[4].clone()))
        .collect();
    // Rows are in original order; find the amount=300 row → rn=3, cum=600.
    let r300 = emea
        .iter()
        .find(|(a, _, _)| *a == DataValue::Int(300))
        .unwrap();
    assert_eq!(r300.1, DataValue::Int(3));
    assert_eq!(r300.2, DataValue::Int(600));
    let r100 = emea
        .iter()
        .find(|(a, _, _)| *a == DataValue::Int(100))
        .unwrap();
    assert_eq!(r100.1, DataValue::Int(1));
    assert_eq!(r100.2, DataValue::Int(100));
}
