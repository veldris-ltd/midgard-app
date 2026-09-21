//! Engine: transform execution, deterministic re-runs (AC-F-2), type inference,
//! fail-closed type errors, and the mock-backend seam. [FR-F-1] [FR-F-5]

use forge_dataset_store::{Column, ColumnType, DataValue, Dataset, Schema};
use forge_engine::{
    BinOp, CmpOp, ComputeBackend, ExecutionPlan, Expr, MockBackend, Predicate, ReferenceBackend,
    Transform,
};

fn schema() -> Schema {
    Schema::new(vec![
        Column::new("region", ColumnType::Text),
        Column::new("units", ColumnType::Int),
        Column::new("price", ColumnType::Float),
    ])
}

fn dataset() -> Dataset {
    Dataset::new(
        schema(),
        vec![
            vec![
                DataValue::Text("emea".into()),
                DataValue::Int(120),
                DataValue::Float(2.5),
            ],
            vec![
                DataValue::Text("amer".into()),
                DataValue::Int(340),
                DataValue::Float(3.0),
            ],
            vec![
                DataValue::Text("apac".into()),
                DataValue::Int(90),
                DataValue::Float(1.5),
            ],
        ],
    )
    .unwrap()
}

#[test]
fn filter_derive_select_pipeline_executes_correctly() {
    // Keep units >= 100, derive revenue = units_as_float? — here revenue = price*price
    // is nonsense; instead derive doubled = units + units, then select region+doubled.
    let plan = ExecutionPlan::new(vec![
        Transform::Filter(Predicate::Compare {
            column: "units".into(),
            op: CmpOp::Ge,
            value: DataValue::Int(100),
        }),
        Transform::Derive {
            name: "doubled".into(),
            expr: Expr::Binary(
                BinOp::Add,
                Box::new(Expr::Column("units".into())),
                Box::new(Expr::Column("units".into())),
            ),
            ty: ColumnType::Int,
        },
        Transform::Select(vec!["region".into(), "doubled".into()]),
    ]);

    let out = ReferenceBackend::new().execute(&plan, &dataset()).unwrap();
    assert_eq!(out.schema().width(), 2);
    assert_eq!(out.row_count(), 2); // emea(120) and amer(340) survive; apac(90) filtered
    assert_eq!(out.rows()[0][0], DataValue::Text("emea".into()));
    assert_eq!(out.rows()[0][1], DataValue::Int(240));
    assert_eq!(out.rows()[1][1], DataValue::Int(680));
}

#[test]
fn execution_is_deterministic() {
    // AC-F-2 foundation: identical plan + input → byte-identical output every run.
    let plan = ExecutionPlan::new(vec![
        Transform::Filter(Predicate::Compare {
            column: "region".into(),
            op: CmpOp::Ne,
            value: DataValue::Text("apac".into()),
        }),
        Transform::Cast {
            column: "units".into(),
            to: ColumnType::Float,
        },
    ]);
    let backend = ReferenceBackend::new();
    let a = backend.execute(&plan, &dataset()).unwrap();
    let b = backend.execute(&plan, &dataset()).unwrap();
    assert_eq!(a, b);
    // units is now a float column.
    assert_eq!(a.schema().column("units").unwrap().ty, ColumnType::Float);
    assert_eq!(a.rows()[0][1], DataValue::Float(120.0));
}

#[test]
fn output_schema_infers_without_executing() {
    let plan = ExecutionPlan::new(vec![
        Transform::Derive {
            name: "revenue".into(),
            expr: Expr::Binary(
                BinOp::Mul,
                Box::new(Expr::Column("price".into())),
                Box::new(Expr::Column("price".into())),
            ),
            ty: ColumnType::Float,
        },
        Transform::Select(vec!["region".into(), "revenue".into()]),
    ]);
    let out = plan.output_schema(&schema()).unwrap();
    assert_eq!(out.width(), 2);
    assert_eq!(out.column("revenue").unwrap().ty, ColumnType::Float);
}

#[test]
fn type_errors_fail_closed_before_execution() {
    // Unknown column.
    let p1 = ExecutionPlan::new(vec![Transform::Select(vec!["nope".into()])]);
    assert!(p1.output_schema(&schema()).is_err());

    // Arithmetic on a text column.
    let p2 = ExecutionPlan::new(vec![Transform::Derive {
        name: "bad".into(),
        expr: Expr::Binary(
            BinOp::Add,
            Box::new(Expr::Column("region".into())),
            Box::new(Expr::Lit(DataValue::Int(1))),
        ),
        ty: ColumnType::Int,
    }]);
    assert!(p2.output_schema(&schema()).is_err());

    // Comparing an int column to a text literal.
    let p3 = ExecutionPlan::new(vec![Transform::Filter(Predicate::Compare {
        column: "units".into(),
        op: CmpOp::Gt,
        value: DataValue::Text("x".into()),
    })]);
    assert!(p3.output_schema(&schema()).is_err());

    // Duplicate derived column name.
    let p4 = ExecutionPlan::new(vec![Transform::Derive {
        name: "region".into(),
        expr: Expr::Lit(DataValue::Text("x".into())),
        ty: ColumnType::Text,
    }]);
    assert!(p4.output_schema(&schema()).is_err());
}

#[test]
fn mock_backend_isolates_the_seam() {
    let plan = ExecutionPlan::new(vec![Transform::Limit(1)]);
    // Passthrough ignores the plan.
    let pass = MockBackend::passthrough();
    assert_eq!(pass.execute(&plan, &dataset()).unwrap(), dataset());
    assert_eq!(pass.name(), "mock");

    // Fixed returns a canned dataset.
    let canned = Dataset::empty(schema());
    let fixed = MockBackend::returning(canned.clone());
    assert_eq!(fixed.execute(&plan, &dataset()).unwrap(), canned);
}

#[test]
fn cast_text_to_int_parses_and_bad_text_fails_closed() {
    let s = Schema::new(vec![Column::new("n", ColumnType::Text)]);
    let good = Dataset::new(s.clone(), vec![vec![DataValue::Text("42".into())]]).unwrap();
    let bad = Dataset::new(s, vec![vec![DataValue::Text("oops".into())]]).unwrap();

    let plan = ExecutionPlan::new(vec![Transform::Cast {
        column: "n".into(),
        to: ColumnType::Int,
    }]);
    let backend = ReferenceBackend::new();
    assert_eq!(
        backend.execute(&plan, &good).unwrap().rows()[0][0],
        DataValue::Int(42)
    );
    assert!(backend.execute(&plan, &bad).is_err());
}
