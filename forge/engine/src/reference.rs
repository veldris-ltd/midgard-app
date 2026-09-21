//! The in-process reference [`ComputeBackend`]: a pure-Rust, deterministic executor
//! over the transform IR. It is the Phase-1 engine and the correctness oracle every
//! future backend (Polars/DuckDB/Spark) must match. No threads, no I/O, no clock —
//! same plan + input → identical output, offline. [FR-F-1] [AC-F-2] [NFR-PRV-1]

use std::cmp::Ordering;
use std::collections::BTreeMap;

use forge_dataset_store::{ColumnType, DataValue, Dataset, Row, Schema};

use crate::backend::ComputeBackend;
use crate::error::EngineError;
use crate::plan::{
    AggFunc, AggregateSpec, BinOp, CmpOp, ExecutionPlan, Expr, JoinType, Predicate, SortKey,
    Transform, WindowFunc,
};

/// The reference executor.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReferenceBackend;

impl ReferenceBackend {
    /// Construct the reference backend.
    pub fn new() -> Self {
        Self
    }
}

impl ComputeBackend for ReferenceBackend {
    fn name(&self) -> &str {
        "reference"
    }

    fn execute(&self, plan: &ExecutionPlan, input: &Dataset) -> Result<Dataset, EngineError> {
        let mut schema = input.schema().clone();
        let mut rows: Vec<Row> = input.rows().to_vec();
        for op in &plan.ops {
            // Type-check against the current schema (also the executor's contract).
            let next_schema = op.apply_schema(&schema)?;
            rows = apply_rows(op, &schema, rows)?;
            schema = next_schema;
        }
        // Re-validate: the executor must produce data honouring the inferred schema.
        Dataset::new(schema, rows).map_err(|e| EngineError::TypeError(e.to_string()))
    }
}

/// Apply one op to the rows, using `schema` (the *input* schema of this op) to
/// resolve column positions.
fn apply_rows(op: &Transform, schema: &Schema, rows: Vec<Row>) -> Result<Vec<Row>, EngineError> {
    match op {
        Transform::Select(cols) => {
            let idxs: Vec<usize> = cols
                .iter()
                .map(|c| {
                    schema
                        .index_of(c)
                        .ok_or_else(|| EngineError::UnknownColumn(c.clone()))
                })
                .collect::<Result<_, _>>()?;
            Ok(rows
                .into_iter()
                .map(|row| idxs.iter().map(|&i| row[i].clone()).collect())
                .collect())
        }
        Transform::Rename { .. } => Ok(rows), // structural only
        Transform::Filter(pred) => Ok(rows
            .into_iter()
            .filter(|row| eval_predicate(pred, schema, row))
            .collect()),
        Transform::Derive { expr, .. } => rows
            .into_iter()
            .map(|mut row| {
                let v = eval_expr(expr, schema, &row)?;
                row.push(v);
                Ok(row)
            })
            .collect(),
        Transform::Cast { column, to } => {
            let idx = schema
                .index_of(column)
                .ok_or_else(|| EngineError::UnknownColumn(column.clone()))?;
            rows.into_iter()
                .map(|mut row| {
                    row[idx] = cast_value(&row[idx], *to)?;
                    Ok(row)
                })
                .collect()
        }
        Transform::Limit(n) => Ok(rows.into_iter().take(*n).collect()),
        Transform::Sort(keys) => {
            let idxs: Vec<usize> = keys
                .iter()
                .map(|k| {
                    schema
                        .index_of(&k.column)
                        .ok_or_else(|| EngineError::UnknownColumn(k.column.clone()))
                })
                .collect::<Result<_, _>>()?;
            let mut rows = rows;
            // Stable multi-key ordering; nulls sort last in ascending order.
            rows.sort_by(|a, b| {
                for (col_i, key) in idxs.iter().zip(keys) {
                    let ord = order_cells(&a[*col_i], &b[*col_i]);
                    let ord = if key.descending { ord.reverse() } else { ord };
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
                Ordering::Equal
            });
            Ok(rows)
        }
        Transform::Aggregate { group_by, aggs } => execute_aggregate(schema, &rows, group_by, aggs),
        Transform::Join {
            right,
            left_on,
            right_on,
            how,
        } => execute_join(schema, rows, right, left_on, right_on, *how),
        Transform::Window {
            partition_by,
            order_by,
            func,
            output: _,
        } => execute_window(schema, rows, partition_by, order_by, func),
    }
}

/// A total order over cells for sort/window: same-typed values compare naturally;
/// Null is greatest (sorts last ascending); cross-type (shouldn't occur post
/// type-check) falls back to Equal.
fn order_cells(a: &DataValue, b: &DataValue) -> Ordering {
    match (a, b) {
        (DataValue::Null, DataValue::Null) => Ordering::Equal,
        (DataValue::Null, _) => Ordering::Greater,
        (_, DataValue::Null) => Ordering::Less,
        _ => compare_values(a, b).unwrap_or(Ordering::Equal),
    }
}

/// A deterministic byte encoding of a key tuple, so groups/joins are order-stable
/// (a `BTreeMap` over these keys iterates in a fixed order). Also used by the
/// partitioned backend to hash-shuffle rows by key.
pub(crate) fn encode_key(cells: &[&DataValue]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in cells {
        match c {
            DataValue::Null => out.push(0),
            DataValue::Int(i) => {
                out.push(1);
                out.extend_from_slice(&i.to_le_bytes());
            }
            DataValue::Float(f) => {
                out.push(2);
                out.extend_from_slice(&f.to_bits().to_le_bytes());
            }
            DataValue::Text(s) => {
                out.push(3);
                out.extend_from_slice(&(s.len() as u64).to_le_bytes());
                out.extend_from_slice(s.as_bytes());
            }
            DataValue::Bool(b) => {
                out.push(4);
                out.push(*b as u8);
            }
            DataValue::Bytes(b) => {
                out.push(5);
                out.extend_from_slice(&(b.len() as u64).to_le_bytes());
                out.extend_from_slice(b);
            }
        }
    }
    out
}

fn key_has_null(cells: &[&DataValue]) -> bool {
    cells.iter().any(|c| matches!(c, DataValue::Null))
}

/// Group rows by `group_by` and fold each aggregate. Output rows are ordered by the
/// encoded group key (deterministic), each = group-key cells ++ aggregate cells.
fn execute_aggregate(
    schema: &Schema,
    rows: &[Row],
    group_by: &[String],
    aggs: &[AggregateSpec],
) -> Result<Vec<Row>, EngineError> {
    let key_idx: Vec<usize> = group_by
        .iter()
        .map(|c| {
            schema
                .index_of(c)
                .ok_or_else(|| EngineError::UnknownColumn(c.clone()))
        })
        .collect::<Result<_, _>>()?;

    // Preserve group insertion order under a stable key sort: collect member row
    // indices per encoded key.
    let mut groups: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<&DataValue> = key_idx.iter().map(|&k| &row[k]).collect();
        groups.entry(encode_key(&cells)).or_default().push(i);
    }

    let mut out = Vec::with_capacity(groups.len().max(1));
    // Empty group-by with zero rows still yields one global (empty) group only if
    // there are aggregates that define over-empty values; SQL yields one row for a
    // global aggregate even with no input, but our datasets are non-empty in
    // practice. We emit a global row when group_by is empty.
    if group_by.is_empty() && rows.is_empty() {
        let mut cells = Vec::new();
        for spec in aggs {
            cells.push(fold_aggregate(&spec.func, schema, rows, &[])?);
        }
        out.push(cells);
        return Ok(out);
    }
    for member_idxs in groups.values() {
        let first = &rows[member_idxs[0]];
        let mut cells: Vec<DataValue> = key_idx.iter().map(|&k| first[k].clone()).collect();
        for spec in aggs {
            cells.push(fold_aggregate(&spec.func, schema, rows, member_idxs)?);
        }
        out.push(cells);
    }
    // Order the output by the group-key columns in NATURAL value order (the encoded
    // key is only used for grouping, not ordering), so the result is deterministic
    // and intuitive — and identical to what a partitioned backend must produce.
    let k = group_by.len();
    out.sort_by(|a, b| {
        for i in 0..k {
            let ord = order_cells(&a[i], &b[i]);
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    });
    Ok(out)
}

/// Fold one aggregate over the group's member rows (nulls ignored, SQL-style).
fn fold_aggregate(
    func: &AggFunc,
    schema: &Schema,
    rows: &[Row],
    members: &[usize],
) -> Result<DataValue, EngineError> {
    let col_idx = |c: &str| {
        schema
            .index_of(c)
            .ok_or_else(|| EngineError::UnknownColumn(c.to_string()))
    };
    match func {
        AggFunc::Count => Ok(DataValue::Int(members.len() as i64)),
        AggFunc::Sum(c) => {
            let i = col_idx(c)?;
            let mut int_sum = 0i64;
            let mut float_sum = 0f64;
            let mut is_float = false;
            for &m in members {
                match &rows[m][i] {
                    DataValue::Int(v) => int_sum += v,
                    DataValue::Float(v) => {
                        is_float = true;
                        float_sum += v;
                    }
                    DataValue::Null => {}
                    _ => return Err(EngineError::TypeError("sum over non-numeric".into())),
                }
            }
            Ok(if is_float {
                DataValue::Float(float_sum + int_sum as f64)
            } else {
                DataValue::Int(int_sum)
            })
        }
        AggFunc::Avg(c) => {
            let i = col_idx(c)?;
            let mut sum = 0f64;
            let mut n = 0u64;
            for &m in members {
                match &rows[m][i] {
                    DataValue::Int(v) => {
                        sum += *v as f64;
                        n += 1;
                    }
                    DataValue::Float(v) => {
                        sum += v;
                        n += 1;
                    }
                    DataValue::Null => {}
                    _ => return Err(EngineError::TypeError("avg over non-numeric".into())),
                }
            }
            Ok(DataValue::Float(if n == 0 { 0.0 } else { sum / n as f64 }))
        }
        AggFunc::Min(c) | AggFunc::Max(c) => {
            let i = col_idx(c)?;
            let want_max = matches!(func, AggFunc::Max(_));
            let mut best: Option<&DataValue> = None;
            for &m in members {
                let v = &rows[m][i];
                if matches!(v, DataValue::Null) {
                    continue;
                }
                best = Some(match best {
                    None => v,
                    Some(cur) => {
                        let ord = compare_values(v, cur).unwrap_or(Ordering::Equal);
                        if (want_max && ord == Ordering::Greater)
                            || (!want_max && ord == Ordering::Less)
                        {
                            v
                        } else {
                            cur
                        }
                    }
                });
            }
            Ok(best.cloned().unwrap_or(DataValue::Null))
        }
    }
}

/// Hash-equi-join `rows` against the embedded `right` dataset. Deterministic: left
/// order preserved, right matches in right order.
fn execute_join(
    schema: &Schema,
    rows: Vec<Row>,
    right: &Dataset,
    left_on: &[String],
    right_on: &[String],
    how: JoinType,
) -> Result<Vec<Row>, EngineError> {
    let l_idx: Vec<usize> = left_on
        .iter()
        .map(|c| {
            schema
                .index_of(c)
                .ok_or_else(|| EngineError::UnknownColumn(c.clone()))
        })
        .collect::<Result<_, _>>()?;
    let rschema = right.schema();
    let r_key_idx: Vec<usize> = right_on
        .iter()
        .map(|c| {
            rschema
                .index_of(c)
                .ok_or_else(|| EngineError::UnknownColumn(c.clone()))
        })
        .collect::<Result<_, _>>()?;
    // The right columns carried into the output (all but the right keys), in order.
    let r_out_idx: Vec<usize> = (0..rschema.width())
        .filter(|i| !r_key_idx.contains(i))
        .collect();
    let r_out_width = r_out_idx.len();

    // Build the hash side on the right keys.
    let mut index: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (i, row) in right.rows().iter().enumerate() {
        let cells: Vec<&DataValue> = r_key_idx.iter().map(|&k| &row[k]).collect();
        if key_has_null(&cells) {
            continue; // null keys never match (SQL semantics)
        }
        index.entry(encode_key(&cells)).or_default().push(i);
    }

    let left_outer = matches!(how, JoinType::Left);
    let mut out = Vec::new();
    for lrow in rows {
        let cells: Vec<&DataValue> = l_idx.iter().map(|&k| &lrow[k]).collect();
        let matches = if key_has_null(&cells) {
            None
        } else {
            index.get(&encode_key(&cells))
        };
        match matches {
            Some(m) => {
                for &ri in m {
                    let rrow = &right.rows()[ri];
                    let mut joined = lrow.clone();
                    for &c in &r_out_idx {
                        joined.push(rrow[c].clone());
                    }
                    out.push(joined);
                }
            }
            None => {
                if left_outer {
                    let mut joined = lrow;
                    joined.extend(std::iter::repeat(DataValue::Null).take(r_out_width));
                    out.push(joined);
                }
            }
        }
    }
    Ok(out)
}

/// Append a window-function value per row, computed within `partition_by` groups in
/// `order_by` order. Rows are emitted in their ORIGINAL order with the value pushed.
fn execute_window(
    schema: &Schema,
    rows: Vec<Row>,
    partition_by: &[String],
    order_by: &[SortKey],
    func: &WindowFunc,
) -> Result<Vec<Row>, EngineError> {
    let p_idx: Vec<usize> = partition_by
        .iter()
        .map(|c| {
            schema
                .index_of(c)
                .ok_or_else(|| EngineError::UnknownColumn(c.clone()))
        })
        .collect::<Result<_, _>>()?;
    let o_idx: Vec<usize> = order_by
        .iter()
        .map(|k| {
            schema
                .index_of(&k.column)
                .ok_or_else(|| EngineError::UnknownColumn(k.column.clone()))
        })
        .collect::<Result<_, _>>()?;
    let sum_idx = match func {
        WindowFunc::RunningSum(c) => Some(
            schema
                .index_of(c)
                .ok_or_else(|| EngineError::UnknownColumn(c.clone()))?,
        ),
        _ => None,
    };

    // Partition original row indices.
    let mut parts: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<&DataValue> = p_idx.iter().map(|&k| &row[k]).collect();
        parts.entry(encode_key(&cells)).or_default().push(i);
    }

    let mut value_for: Vec<DataValue> = vec![DataValue::Null; rows.len()];
    for members in parts.values() {
        // Stable order within the partition by the window's order keys.
        let mut ordered = members.clone();
        ordered.sort_by(|&a, &b| {
            for (col_i, key) in o_idx.iter().zip(order_by) {
                let ord = order_cells(&rows[a][*col_i], &rows[b][*col_i]);
                let ord = if key.descending { ord.reverse() } else { ord };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            Ordering::Equal
        });

        let mut running_i = 0i64;
        let mut running_f = 0f64;
        let mut running_float = false;
        let mut rank = 0i64;
        let mut prev_key: Option<Vec<u8>> = None;
        for (pos, &orig) in ordered.iter().enumerate() {
            let v = match func {
                WindowFunc::RowNumber => DataValue::Int(pos as i64 + 1),
                WindowFunc::Rank => {
                    let key_cells: Vec<&DataValue> =
                        o_idx.iter().map(|&k| &rows[orig][k]).collect();
                    let k = encode_key(&key_cells);
                    if prev_key.as_ref() != Some(&k) {
                        rank = pos as i64 + 1;
                        prev_key = Some(k);
                    }
                    DataValue::Int(rank)
                }
                WindowFunc::RunningSum(_) => {
                    match &rows[orig][sum_idx.unwrap()] {
                        DataValue::Int(x) => running_i += x,
                        DataValue::Float(x) => {
                            running_float = true;
                            running_f += x;
                        }
                        DataValue::Null => {}
                        _ => {
                            return Err(EngineError::TypeError(
                                "running sum over non-numeric".into(),
                            ))
                        }
                    }
                    if running_float {
                        DataValue::Float(running_f + running_i as f64)
                    } else {
                        DataValue::Int(running_i)
                    }
                }
            };
            value_for[orig] = v;
        }
    }

    let out = rows
        .into_iter()
        .enumerate()
        .map(|(i, mut row)| {
            row.push(value_for[i].clone());
            row
        })
        .collect();
    Ok(out)
}

/// Evaluate an expression against a row.
fn eval_expr(expr: &Expr, schema: &Schema, row: &Row) -> Result<DataValue, EngineError> {
    match expr {
        Expr::Column(name) => {
            let idx = schema
                .index_of(name)
                .ok_or_else(|| EngineError::UnknownColumn(name.clone()))?;
            Ok(row[idx].clone())
        }
        Expr::Lit(v) => Ok(v.clone()),
        Expr::Binary(op, a, b) => {
            let va = eval_expr(a, schema, row)?;
            let vb = eval_expr(b, schema, row)?;
            // Null propagates (the derived column is typed nullable accordingly).
            if matches!(va, DataValue::Null) || matches!(vb, DataValue::Null) {
                return Ok(DataValue::Null);
            }
            match (op, va, vb) {
                (BinOp::Add, DataValue::Int(x), DataValue::Int(y)) => Ok(DataValue::Int(x + y)),
                (BinOp::Sub, DataValue::Int(x), DataValue::Int(y)) => Ok(DataValue::Int(x - y)),
                (BinOp::Mul, DataValue::Int(x), DataValue::Int(y)) => Ok(DataValue::Int(x * y)),
                (BinOp::Add, DataValue::Float(x), DataValue::Float(y)) => {
                    Ok(DataValue::Float(x + y))
                }
                (BinOp::Sub, DataValue::Float(x), DataValue::Float(y)) => {
                    Ok(DataValue::Float(x - y))
                }
                (BinOp::Mul, DataValue::Float(x), DataValue::Float(y)) => {
                    Ok(DataValue::Float(x * y))
                }
                (BinOp::Concat, DataValue::Text(x), DataValue::Text(y)) => {
                    Ok(DataValue::Text(format!("{x}{y}")))
                }
                (_, x, y) => Err(EngineError::TypeError(format!(
                    "runtime type mismatch in binary op: {x:?} / {y:?}"
                ))),
            }
        }
    }
}

/// Evaluate a predicate against a row. Any comparison involving a null is false
/// (SQL-style three-valued logic collapsed to reject).
fn eval_predicate(pred: &Predicate, schema: &Schema, row: &Row) -> bool {
    match pred {
        Predicate::Compare { column, op, value } => {
            let idx = match schema.index_of(column) {
                Some(i) => i,
                None => return false,
            };
            let cell = &row[idx];
            if matches!(cell, DataValue::Null) || matches!(value, DataValue::Null) {
                return false;
            }
            // One exhaustive match over the operator. Equality compares cells
            // directly; the ordering operators evaluate `compare_values` (a `None`
            // result — cross-type, ruled out by the type-checker — rejects). No
            // nested re-match, so no unreachable arm. [RT-54]
            match op {
                CmpOp::Eq => cell == value,
                CmpOp::Ne => cell != value,
                CmpOp::Lt => compare_values(cell, value) == Some(Ordering::Less),
                CmpOp::Le => matches!(
                    compare_values(cell, value),
                    Some(Ordering::Less | Ordering::Equal)
                ),
                CmpOp::Gt => compare_values(cell, value) == Some(Ordering::Greater),
                CmpOp::Ge => matches!(
                    compare_values(cell, value),
                    Some(Ordering::Greater | Ordering::Equal)
                ),
            }
        }
        Predicate::And(a, b) => eval_predicate(a, schema, row) && eval_predicate(b, schema, row),
        Predicate::Or(a, b) => eval_predicate(a, schema, row) || eval_predicate(b, schema, row),
        Predicate::Not(p) => !eval_predicate(p, schema, row),
    }
}

/// Total order for same-typed comparable values (ints, floats, text).
fn compare_values(a: &DataValue, b: &DataValue) -> Option<Ordering> {
    match (a, b) {
        (DataValue::Int(x), DataValue::Int(y)) => Some(x.cmp(y)),
        (DataValue::Float(x), DataValue::Float(y)) => x.partial_cmp(y),
        (DataValue::Text(x), DataValue::Text(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

/// Convert a value to a target type by a permitted cast. Nulls pass through.
fn cast_value(v: &DataValue, to: ColumnType) -> Result<DataValue, EngineError> {
    if matches!(v, DataValue::Null) {
        return Ok(DataValue::Null);
    }
    let out = match (v, to) {
        (DataValue::Int(i), ColumnType::Int) => DataValue::Int(*i),
        (DataValue::Int(i), ColumnType::Float) => DataValue::Float(*i as f64),
        (DataValue::Int(i), ColumnType::Text) => DataValue::Text(i.to_string()),
        (DataValue::Float(f), ColumnType::Float) => DataValue::Float(*f),
        (DataValue::Float(f), ColumnType::Int) => DataValue::Int(*f as i64),
        (DataValue::Float(f), ColumnType::Text) => DataValue::Text(f.to_string()),
        (DataValue::Bool(b), ColumnType::Bool) => DataValue::Bool(*b),
        (DataValue::Bool(b), ColumnType::Text) => DataValue::Text(b.to_string()),
        (DataValue::Text(s), ColumnType::Text) => DataValue::Text(s.clone()),
        (DataValue::Text(s), ColumnType::Int) => s
            .trim()
            .parse::<i64>()
            .map(DataValue::Int)
            .map_err(|_| EngineError::TypeError(format!("'{s}' is not an int")))?,
        (DataValue::Text(s), ColumnType::Float) => s
            .trim()
            .parse::<f64>()
            .map(DataValue::Float)
            .map_err(|_| EngineError::TypeError(format!("'{s}' is not a float")))?,
        (DataValue::Text(s), ColumnType::Bool) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => DataValue::Bool(true),
            "false" | "0" | "no" => DataValue::Bool(false),
            _ => return Err(EngineError::TypeError(format!("'{s}' is not a bool"))),
        },
        (val, ty) => {
            return Err(EngineError::TypeError(format!(
                "unsupported cast {val:?} -> {}",
                ty.as_str()
            )))
        }
    };
    Ok(out)
}
