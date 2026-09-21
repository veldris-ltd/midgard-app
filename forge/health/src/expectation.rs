//! Data-health expectations: declarative assertions about a dataset that must hold
//! for a build to be promotable. Each expectation evaluates to a pass/fail outcome
//! with a human-readable detail line — the material a build report shows when it
//! blocks a promotion. [FR-F-6]

use forge_dataset_store::{DataValue, Dataset};

/// One declarative data-quality assertion over a dataset.
#[derive(Debug, Clone, PartialEq)]
pub enum Expectation {
    /// A column contains no nulls.
    NotNull {
        /// Column to check.
        column: String,
    },
    /// A column's non-null values are all distinct.
    Unique {
        /// Column to check.
        column: String,
    },
    /// The dataset has at least `n` rows.
    RowCountAtLeast(usize),
    /// The dataset has at most `n` rows.
    RowCountAtMost(usize),
    /// Every non-null value in an integer column lies within `[min, max]`.
    IntInRange {
        /// Column to check.
        column: String,
        /// Inclusive lower bound.
        min: i64,
        /// Inclusive upper bound.
        max: i64,
    },
}

/// The result of evaluating one expectation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectationOutcome {
    /// A stable description of the expectation.
    pub description: String,
    /// Whether it held.
    pub passed: bool,
    /// A short, non-secret detail (violation count, missing column, …).
    pub detail: String,
}

impl Expectation {
    /// Evaluate this expectation against a dataset.
    pub fn evaluate(&self, ds: &Dataset) -> ExpectationOutcome {
        match self {
            Expectation::NotNull { column } => self.column_check(ds, column, |idx, rows| {
                let violations = rows
                    .iter()
                    .filter(|r| matches!(r[idx], DataValue::Null))
                    .count();
                (
                    violations == 0,
                    format!("{violations} null value(s) in '{column}'"),
                )
            }),
            Expectation::Unique { column } => self.column_check(ds, column, |idx, rows| {
                let mut seen = Vec::new();
                let mut dupes = 0usize;
                for r in rows {
                    if matches!(r[idx], DataValue::Null) {
                        continue;
                    }
                    let key = value_key(&r[idx]);
                    if seen.contains(&key) {
                        dupes += 1;
                    } else {
                        seen.push(key);
                    }
                }
                (
                    dupes == 0,
                    format!("{dupes} duplicate value(s) in '{column}'"),
                )
            }),
            Expectation::RowCountAtLeast(n) => {
                let count = ds.row_count();
                ExpectationOutcome {
                    description: format!("row_count >= {n}"),
                    passed: count >= *n,
                    detail: format!("row_count = {count}"),
                }
            }
            Expectation::RowCountAtMost(n) => {
                let count = ds.row_count();
                ExpectationOutcome {
                    description: format!("row_count <= {n}"),
                    passed: count <= *n,
                    detail: format!("row_count = {count}"),
                }
            }
            Expectation::IntInRange { column, min, max } => {
                self.column_check(ds, column, |idx, rows| {
                    let mut violations = 0usize;
                    for r in rows {
                        match &r[idx] {
                            DataValue::Null => {}
                            DataValue::Int(i) if *i >= *min && *i <= *max => {}
                            _ => violations += 1,
                        }
                    }
                    (
                        violations == 0,
                        format!("{violations} value(s) outside [{min}, {max}] in '{column}'"),
                    )
                })
            }
        }
    }

    /// Shared column-resolution wrapper: resolves the column (failing closed if it
    /// is absent) then runs `f(idx, rows)` for the pass/detail.
    fn column_check<F>(&self, ds: &Dataset, column: &str, f: F) -> ExpectationOutcome
    where
        F: FnOnce(usize, &[Vec<DataValue>]) -> (bool, String),
    {
        let description = self.description();
        match ds.schema().index_of(column) {
            Some(idx) => {
                let (passed, detail) = f(idx, ds.rows());
                ExpectationOutcome {
                    description,
                    passed,
                    detail,
                }
            }
            None => ExpectationOutcome {
                description,
                passed: false,
                detail: format!("column '{column}' does not exist"),
            },
        }
    }

    /// A stable description string.
    pub fn description(&self) -> String {
        match self {
            Expectation::NotNull { column } => format!("not_null({column})"),
            Expectation::Unique { column } => format!("unique({column})"),
            Expectation::RowCountAtLeast(n) => format!("row_count >= {n}"),
            Expectation::RowCountAtMost(n) => format!("row_count <= {n}"),
            Expectation::IntInRange { column, min, max } => {
                format!("int_in_range({column}, {min}, {max})")
            }
        }
    }
}

/// A deterministic string key for value equality (used by the uniqueness check).
fn value_key(v: &DataValue) -> String {
    match v {
        DataValue::Null => "\u{0}null".to_string(),
        DataValue::Bool(b) => format!("b:{b}"),
        DataValue::Int(i) => format!("i:{i}"),
        DataValue::Float(f) => format!("f:{}", f.to_bits()),
        DataValue::Text(s) => format!("t:{s}"),
        DataValue::Bytes(b) => format!("y:{}", b.len()),
    }
}
