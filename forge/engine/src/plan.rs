//! The transform IR and its **type system**. An [`ExecutionPlan`] is an ordered
//! list of [`Transform`]s; [`ExecutionPlan::output_schema`] infers the resulting
//! schema and type-checks every step *without running it*. Both the pro-code IDE
//! and the low-code builder lower to this one IR, and both the type-checker and the
//! executor call the same [`Transform::apply_schema`] — so "will it compile" and
//! "what it computes" can never disagree. [FR-F-1]

use forge_dataset_store::{Column, ColumnType, DataValue, Dataset, Schema};

use crate::error::EngineError;

/// A sort key: a column and a direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    /// Column to order by.
    pub column: String,
    /// Descending if true, else ascending.
    pub descending: bool,
}

impl SortKey {
    /// Ascending on `column`.
    pub fn asc(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            descending: false,
        }
    }
    /// Descending on `column`.
    pub fn desc(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            descending: true,
        }
    }
}

/// An aggregate function. `Count` counts rows in the group; the rest fold a column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggFunc {
    /// Number of rows in the group.
    Count,
    /// Sum of a numeric column.
    Sum(String),
    /// Minimum of an orderable column.
    Min(String),
    /// Maximum of an orderable column.
    Max(String),
    /// Mean of a numeric column (always Float).
    Avg(String),
}

impl AggFunc {
    /// The input column this aggregate reads, if any.
    pub fn input(&self) -> Option<&str> {
        match self {
            AggFunc::Count => None,
            AggFunc::Sum(c) | AggFunc::Min(c) | AggFunc::Max(c) | AggFunc::Avg(c) => Some(c),
        }
    }
}

/// One aggregate output: a function and the name of the column it produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateSpec {
    /// The aggregate function.
    pub func: AggFunc,
    /// The output column name.
    pub output: String,
}

/// How a join treats left rows with no right match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    /// Emit only matched pairs.
    Inner,
    /// Emit every left row; right columns are null when unmatched.
    Left,
}

/// A window function evaluated within a partition, in the window's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowFunc {
    /// 1-based sequence number within the partition.
    RowNumber,
    /// 1-based rank with ties sharing a rank (gaps after ties).
    Rank,
    /// Running sum of a numeric column up to and including the current row.
    RunningSum(String),
}

/// A comparison operator for row predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    /// `=`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

impl CmpOp {
    /// Whether this operator needs an ordering (vs. only equality).
    fn needs_order(self) -> bool {
        !matches!(self, CmpOp::Eq | CmpOp::Ne)
    }
}

/// A boolean row predicate over columns and literals.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// Compare a column against a literal.
    Compare {
        /// Column name.
        column: String,
        /// Operator.
        op: CmpOp,
        /// Literal to compare against.
        value: DataValue,
    },
    /// Both sub-predicates hold.
    And(Box<Predicate>, Box<Predicate>),
    /// Either sub-predicate holds.
    Or(Box<Predicate>, Box<Predicate>),
    /// The sub-predicate does not hold.
    Not(Box<Predicate>),
}

/// A binary operator for derived-column expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// Numeric addition.
    Add,
    /// Numeric subtraction.
    Sub,
    /// Numeric multiplication.
    Mul,
    /// Text concatenation.
    Concat,
}

/// A scalar expression evaluated per row to produce a derived value.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// The value of a column.
    Column(String),
    /// A constant.
    Lit(DataValue),
    /// A binary operation over two sub-expressions.
    Binary(BinOp, Box<Expr>, Box<Expr>),
}

/// One transform step. The set is deliberately small and total — every op is
/// type-inferable and deterministic. This is the seed `TRANSFORM_LIBRARY` the AI
/// assist adapter (Phase 4.d) generates against.
#[derive(Debug, Clone, PartialEq)]
pub enum Transform {
    /// Project to a subset of columns, in the given order.
    Select(Vec<String>),
    /// Rename a column.
    Rename {
        /// Existing column name.
        from: String,
        /// New column name.
        to: String,
    },
    /// Keep only rows matching the predicate.
    Filter(Predicate),
    /// Append a computed column.
    Derive {
        /// New column name.
        name: String,
        /// The expression to evaluate.
        expr: Expr,
        /// The declared result type (checked against the inferred type).
        ty: ColumnType,
    },
    /// Change a column's type by a permitted conversion.
    Cast {
        /// Column to cast.
        column: String,
        /// Target type.
        to: ColumnType,
    },
    /// Keep at most `n` rows.
    Limit(usize),
    /// Order rows by one or more keys (stable, multi-key).
    Sort(Vec<SortKey>),
    /// Group by zero or more keys and compute aggregates per group. The output
    /// schema is the group-by columns followed by the aggregate columns.
    Aggregate {
        /// Grouping columns (empty = one global group).
        group_by: Vec<String>,
        /// The aggregates to compute.
        aggs: Vec<AggregateSpec>,
    },
    /// Equi-join against an embedded right dataset on matching key columns. The
    /// right dataset travels *inside* the plan so the plan stays self-contained and
    /// reproducible. Output = all left columns, then right columns except the right
    /// keys.
    Join {
        /// The right input, embedded in the plan.
        right: Dataset,
        /// Left join-key columns.
        left_on: Vec<String>,
        /// Right join-key columns (paired positionally with `left_on`).
        right_on: Vec<String>,
        /// Inner or left-outer.
        how: JoinType,
    },
    /// Append a window-function column computed within partitions.
    Window {
        /// Partitioning columns (empty = one partition).
        partition_by: Vec<String>,
        /// Ordering within each partition.
        order_by: Vec<SortKey>,
        /// The window function.
        func: WindowFunc,
        /// Output column name.
        output: String,
    },
}

impl Transform {
    /// Whether this op is a row filter — used by the optimiser for predicate
    /// pushdown (filters can move ahead of column-only transforms).
    pub fn is_filter(&self) -> bool {
        matches!(self, Transform::Filter(_))
    }

    /// Whether this op is purely column-structural (does not add/remove/limit rows)
    /// and preserves the columns a preceding filter would reference by name. A
    /// filter may be pushed *ahead* of such an op only if the op does not rename or
    /// drop the filter's columns — callers verify that separately.
    pub fn is_row_preserving(&self) -> bool {
        matches!(
            self,
            Transform::Rename { .. } | Transform::Derive { .. } | Transform::Cast { .. }
        )
    }

    /// Infer the output schema of applying this transform to `schema`, type-checking
    /// as it goes. Pure — no data touched.
    pub fn apply_schema(&self, schema: &Schema) -> Result<Schema, EngineError> {
        match self {
            Transform::Select(cols) => {
                let mut out = Vec::with_capacity(cols.len());
                let mut seen = Vec::new();
                for name in cols {
                    if seen.contains(name) {
                        return Err(EngineError::DuplicateColumn(name.clone()));
                    }
                    let col = schema
                        .column(name)
                        .ok_or_else(|| EngineError::UnknownColumn(name.clone()))?;
                    seen.push(name.clone());
                    out.push(col.clone());
                }
                Ok(Schema::new(out))
            }
            Transform::Rename { from, to } => {
                if schema.index_of(from).is_none() {
                    return Err(EngineError::UnknownColumn(from.clone()));
                }
                if to != from && schema.index_of(to).is_some() {
                    return Err(EngineError::DuplicateColumn(to.clone()));
                }
                let columns = schema
                    .columns
                    .iter()
                    .map(|c| {
                        if &c.name == from {
                            Column {
                                name: to.clone(),
                                ty: c.ty,
                                nullable: c.nullable,
                            }
                        } else {
                            c.clone()
                        }
                    })
                    .collect();
                Ok(Schema::new(columns))
            }
            Transform::Filter(pred) => {
                validate_predicate(pred, schema)?;
                Ok(schema.clone())
            }
            Transform::Derive { name, expr, ty } => {
                if schema.index_of(name).is_some() {
                    return Err(EngineError::DuplicateColumn(name.clone()));
                }
                let inferred = infer_type(expr, schema)?;
                if inferred != *ty {
                    return Err(EngineError::TypeError(format!(
                        "derived column '{name}' declared {} but expression is {}",
                        ty.as_str(),
                        inferred.as_str()
                    )));
                }
                // A derived column is nullable iff its expression can observe a null
                // (references a nullable column) — so a runtime null is well-typed.
                let nullable = expr_nullable(expr, schema);
                let mut columns = schema.columns.clone();
                columns.push(Column {
                    name: name.clone(),
                    ty: *ty,
                    nullable,
                });
                Ok(Schema::new(columns))
            }
            Transform::Cast { column, to } => {
                let col = schema
                    .column(column)
                    .ok_or_else(|| EngineError::UnknownColumn(column.clone()))?;
                if !cast_allowed(col.ty, *to) {
                    return Err(EngineError::TypeError(format!(
                        "cannot cast column '{column}' from {} to {}",
                        col.ty.as_str(),
                        to.as_str()
                    )));
                }
                let columns = schema
                    .columns
                    .iter()
                    .map(|c| {
                        if &c.name == column {
                            Column {
                                name: c.name.clone(),
                                ty: *to,
                                nullable: c.nullable,
                            }
                        } else {
                            c.clone()
                        }
                    })
                    .collect();
                Ok(Schema::new(columns))
            }
            Transform::Limit(_) => Ok(schema.clone()),
            Transform::Sort(keys) => {
                if keys.is_empty() {
                    return Err(EngineError::TypeError("sort needs at least one key".into()));
                }
                for k in keys {
                    let col = schema
                        .column(&k.column)
                        .ok_or_else(|| EngineError::UnknownColumn(k.column.clone()))?;
                    if !is_orderable(col.ty) {
                        return Err(EngineError::TypeError(format!(
                            "cannot sort by column '{}' ({}, not orderable)",
                            k.column,
                            col.ty.as_str()
                        )));
                    }
                }
                Ok(schema.clone()) // sort is schema-preserving
            }
            Transform::Aggregate { group_by, aggs } => {
                let mut out: Vec<Column> = Vec::new();
                let mut seen: Vec<String> = Vec::new();
                for name in group_by {
                    let col = schema
                        .column(name)
                        .ok_or_else(|| EngineError::UnknownColumn(name.clone()))?;
                    if seen.contains(name) {
                        return Err(EngineError::DuplicateColumn(name.clone()));
                    }
                    seen.push(name.clone());
                    out.push(col.clone());
                }
                if aggs.is_empty() {
                    return Err(EngineError::TypeError(
                        "aggregate needs at least one aggregate function".into(),
                    ));
                }
                for spec in aggs {
                    if seen.contains(&spec.output) {
                        return Err(EngineError::DuplicateColumn(spec.output.clone()));
                    }
                    let ty = agg_output_type(&spec.func, schema)?;
                    seen.push(spec.output.clone());
                    // Count/Sum/Avg over a non-empty group are non-null; Min/Max
                    // inherit the source column's nullability.
                    let nullable = match &spec.func {
                        AggFunc::Min(c) | AggFunc::Max(c) => {
                            schema.column(c).map(|x| x.nullable).unwrap_or(false)
                        }
                        _ => false,
                    };
                    out.push(Column {
                        name: spec.output.clone(),
                        ty,
                        nullable,
                    });
                }
                Ok(Schema::new(out))
            }
            Transform::Join {
                right,
                left_on,
                right_on,
                how,
            } => {
                if left_on.len() != right_on.len() || left_on.is_empty() {
                    return Err(EngineError::TypeError(
                        "join needs a non-empty, equal-length key pairing".into(),
                    ));
                }
                let rschema = right.schema();
                for (l, r) in left_on.iter().zip(right_on) {
                    let lc = schema
                        .column(l)
                        .ok_or_else(|| EngineError::UnknownColumn(l.clone()))?;
                    let rc = rschema
                        .column(r)
                        .ok_or_else(|| EngineError::UnknownColumn(r.clone()))?;
                    if lc.ty != rc.ty {
                        return Err(EngineError::TypeError(format!(
                            "join key type mismatch: left '{l}' ({}) vs right '{r}' ({})",
                            lc.ty.as_str(),
                            rc.ty.as_str()
                        )));
                    }
                }
                // Output = all left columns, then right columns except the right keys.
                let mut out = schema.columns.clone();
                let mut names: Vec<String> = out.iter().map(|c| c.name.clone()).collect();
                let left_outer = matches!(how, JoinType::Left);
                for c in &rschema.columns {
                    if right_on.iter().any(|k| k == &c.name) {
                        continue; // right key column is redundant with the left key
                    }
                    if names.contains(&c.name) {
                        return Err(EngineError::DuplicateColumn(c.name.clone()));
                    }
                    names.push(c.name.clone());
                    out.push(Column {
                        name: c.name.clone(),
                        ty: c.ty,
                        // A left join can leave right columns null on unmatched rows.
                        nullable: c.nullable || left_outer,
                    });
                }
                Ok(Schema::new(out))
            }
            Transform::Window {
                partition_by,
                order_by,
                func,
                output,
            } => {
                for name in partition_by {
                    if schema.index_of(name).is_none() {
                        return Err(EngineError::UnknownColumn(name.clone()));
                    }
                }
                for k in order_by {
                    let col = schema
                        .column(&k.column)
                        .ok_or_else(|| EngineError::UnknownColumn(k.column.clone()))?;
                    if !is_orderable(col.ty) {
                        return Err(EngineError::TypeError(format!(
                            "cannot order window by '{}' ({})",
                            k.column,
                            col.ty.as_str()
                        )));
                    }
                }
                if schema.index_of(output).is_some() {
                    return Err(EngineError::DuplicateColumn(output.clone()));
                }
                let ty = match func {
                    WindowFunc::RowNumber | WindowFunc::Rank => ColumnType::Int,
                    WindowFunc::RunningSum(c) => {
                        let col = schema
                            .column(c)
                            .ok_or_else(|| EngineError::UnknownColumn(c.clone()))?;
                        if !is_numeric(col.ty) {
                            return Err(EngineError::TypeError(format!(
                                "running sum requires a numeric column, got '{c}' ({})",
                                col.ty.as_str()
                            )));
                        }
                        col.ty
                    }
                };
                let mut columns = schema.columns.clone();
                columns.push(Column {
                    name: output.clone(),
                    ty,
                    nullable: false,
                });
                Ok(Schema::new(columns))
            }
        }
    }
}

/// Whether a type has a defined total order (used by sort / window / min / max).
fn is_orderable(ty: ColumnType) -> bool {
    matches!(ty, ColumnType::Int | ColumnType::Float | ColumnType::Text)
}

/// Whether a type is numeric (used by sum / avg / running-sum).
fn is_numeric(ty: ColumnType) -> bool {
    matches!(ty, ColumnType::Int | ColumnType::Float)
}

/// The output type of an aggregate function, type-checking its input column.
fn agg_output_type(func: &AggFunc, schema: &Schema) -> Result<ColumnType, EngineError> {
    match func {
        AggFunc::Count => Ok(ColumnType::Int),
        AggFunc::Sum(c) => {
            let col = schema
                .column(c)
                .ok_or_else(|| EngineError::UnknownColumn(c.clone()))?;
            if !is_numeric(col.ty) {
                return Err(EngineError::TypeError(format!(
                    "sum requires a numeric column, got '{c}' ({})",
                    col.ty.as_str()
                )));
            }
            Ok(col.ty)
        }
        AggFunc::Avg(c) => {
            let col = schema
                .column(c)
                .ok_or_else(|| EngineError::UnknownColumn(c.clone()))?;
            if !is_numeric(col.ty) {
                return Err(EngineError::TypeError(format!(
                    "avg requires a numeric column, got '{c}' ({})",
                    col.ty.as_str()
                )));
            }
            Ok(ColumnType::Float)
        }
        AggFunc::Min(c) | AggFunc::Max(c) => {
            let col = schema
                .column(c)
                .ok_or_else(|| EngineError::UnknownColumn(c.clone()))?;
            if !is_orderable(col.ty) {
                return Err(EngineError::TypeError(format!(
                    "min/max requires an orderable column, got '{c}' ({})",
                    col.ty.as_str()
                )));
            }
            Ok(col.ty)
        }
    }
}

/// An ordered transform program.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExecutionPlan {
    /// The steps, applied in order.
    pub ops: Vec<Transform>,
}

impl ExecutionPlan {
    /// A plan from a list of ops.
    pub fn new(ops: Vec<Transform>) -> Self {
        Self { ops }
    }

    /// Infer and type-check the output schema for a given input schema, without
    /// executing. This is the pipeline "type system" entry point. [FR-F-1]
    pub fn output_schema(&self, input: &Schema) -> Result<Schema, EngineError> {
        let mut schema = input.clone();
        for op in &self.ops {
            schema = op.apply_schema(&schema)?;
        }
        Ok(schema)
    }
}

/// The static transform library descriptor: the op vocabulary the AI-assist
/// adapter (4.d) is constrained to. Kept beside the IR so the two never drift.
pub struct TransformLibrary;

impl TransformLibrary {
    /// The available op names.
    pub fn op_names() -> &'static [&'static str] {
        &[
            "select",
            "rename",
            "filter",
            "derive",
            "cast",
            "limit",
            "sort",
            "aggregate",
            "join",
            "window",
        ]
    }
}

/// Infer the type of an expression, type-checking operands.
fn infer_type(expr: &Expr, schema: &Schema) -> Result<ColumnType, EngineError> {
    match expr {
        Expr::Column(name) => schema
            .column(name)
            .map(|c| c.ty)
            .ok_or_else(|| EngineError::UnknownColumn(name.clone())),
        Expr::Lit(v) => lit_type(v),
        Expr::Binary(op, a, b) => {
            let ta = infer_type(a, schema)?;
            let tb = infer_type(b, schema)?;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul => match (ta, tb) {
                    (ColumnType::Int, ColumnType::Int) => Ok(ColumnType::Int),
                    (ColumnType::Float, ColumnType::Float) => Ok(ColumnType::Float),
                    _ => Err(EngineError::TypeError(format!(
                        "arithmetic requires two ints or two floats, got {} and {}",
                        ta.as_str(),
                        tb.as_str()
                    ))),
                },
                BinOp::Concat => match (ta, tb) {
                    (ColumnType::Text, ColumnType::Text) => Ok(ColumnType::Text),
                    _ => Err(EngineError::TypeError(format!(
                        "concat requires two texts, got {} and {}",
                        ta.as_str(),
                        tb.as_str()
                    ))),
                },
            }
        }
    }
}

/// Whether an expression can evaluate to null (references any nullable column).
fn expr_nullable(expr: &Expr, schema: &Schema) -> bool {
    match expr {
        Expr::Column(name) => schema.column(name).map(|c| c.nullable).unwrap_or(false),
        Expr::Lit(_) => false,
        Expr::Binary(_, a, b) => expr_nullable(a, schema) || expr_nullable(b, schema),
    }
}

/// The column type a literal inhabits (a null literal has no type — rejected).
fn lit_type(v: &DataValue) -> Result<ColumnType, EngineError> {
    match v {
        DataValue::Int(_) => Ok(ColumnType::Int),
        DataValue::Float(_) => Ok(ColumnType::Float),
        DataValue::Text(_) => Ok(ColumnType::Text),
        DataValue::Bool(_) => Ok(ColumnType::Bool),
        DataValue::Bytes(_) => Ok(ColumnType::Bytes),
        DataValue::Null => Err(EngineError::TypeError(
            "null literal is not permitted in an expression".into(),
        )),
    }
}

/// Validate a predicate against a schema (columns exist, operators/types agree).
fn validate_predicate(pred: &Predicate, schema: &Schema) -> Result<(), EngineError> {
    match pred {
        Predicate::Compare { column, op, value } => {
            let col = schema
                .column(column)
                .ok_or_else(|| EngineError::UnknownColumn(column.clone()))?;
            let vty = lit_type(value)?;
            if vty != col.ty {
                return Err(EngineError::TypeError(format!(
                    "cannot compare column '{column}' ({}) against a {}",
                    col.ty.as_str(),
                    vty.as_str()
                )));
            }
            if op.needs_order() && matches!(col.ty, ColumnType::Bool | ColumnType::Bytes) {
                return Err(EngineError::TypeError(format!(
                    "ordering comparison not defined for column '{column}' ({})",
                    col.ty.as_str()
                )));
            }
            Ok(())
        }
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            validate_predicate(a, schema)?;
            validate_predicate(b, schema)
        }
        Predicate::Not(p) => validate_predicate(p, schema),
    }
}

/// Whether a cast between two column types is permitted.
pub(crate) fn cast_allowed(from: ColumnType, to: ColumnType) -> bool {
    use ColumnType::*;
    if from == to {
        return true;
    }
    matches!(
        (from, to),
        (Int, Float)
            | (Float, Int)
            | (Int, Text)
            | (Float, Text)
            | (Bool, Text)
            | (Text, Int)
            | (Text, Float)
            | (Text, Bool)
    )
}
