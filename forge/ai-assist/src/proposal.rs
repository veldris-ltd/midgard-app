//! Parse the model's A.6 JSON response into typed transform stages. Every op is
//! mapped onto the engine's [`Transform`] IR; anything the library does not define
//! is a parse error (the model must stay within TRANSFORM_LIBRARY). Column validity
//! is *not* checked here — that is left to the engine type system in the adapter,
//! which is the single source of truth for "does this reference real columns".

use forge_dataset_store::{ColumnType, DataValue};
use forge_engine::{BinOp, CmpOp, Expr, Predicate, Transform};

use crate::error::AssistError;
use crate::json::{self, JsonValue};

/// The structural result of parsing an A.6 response.
#[derive(Debug, Clone, PartialEq)]
pub struct RawProposal {
    /// The proposed transform stages, mapped to the engine IR.
    pub stages: Vec<Transform>,
    /// The model's plain-English explanation.
    pub explanation: String,
    /// Populated iff the model reported it could not satisfy the request.
    pub unsupported: Option<String>,
}

/// Parse an A.6 response document.
pub fn parse_proposal(text: &str) -> Result<RawProposal, AssistError> {
    let root = json::parse(text).map_err(AssistError::Parse)?;

    let explanation = root
        .get("explanation")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let unsupported = match root.get("unsupported") {
        None => None,
        Some(v) if v.is_null() => None,
        Some(v) => v.as_str().map(|s| s.to_string()),
    };

    let mut stages = Vec::new();
    if let Some(arr) = root.get("transform").and_then(|v| v.as_arr()) {
        for op in arr {
            stages.push(parse_op(op)?);
        }
    }

    Ok(RawProposal {
        stages,
        explanation,
        unsupported,
    })
}

/// Map one `{op, args, ...}` object to a typed [`Transform`].
fn parse_op(op: &JsonValue) -> Result<Transform, AssistError> {
    let name = op
        .get("op")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AssistError::Parse("transform op missing 'op'".into()))?;
    let args = op
        .get("args")
        .ok_or_else(|| AssistError::Parse(format!("op '{name}' missing 'args'")))?;

    match name {
        "select" => {
            let cols = args
                .get("columns")
                .and_then(|v| v.as_arr())
                .ok_or_else(|| AssistError::Parse("select.args.columns must be an array".into()))?;
            let columns = cols
                .iter()
                .map(|c| {
                    c.as_str()
                        .map(|s| s.to_string())
                        .ok_or_else(|| AssistError::Parse("select column must be a string".into()))
                })
                .collect::<Result<_, _>>()?;
            Ok(Transform::Select(columns))
        }
        "rename" => Ok(Transform::Rename {
            from: str_arg(args, "from")?,
            to: str_arg(args, "to")?,
        }),
        "filter" => Ok(Transform::Filter(Predicate::Compare {
            column: str_arg(args, "column")?,
            op: cmp_from_str(&str_arg(args, "cmp")?)?,
            value: value_from_json(
                args.get("value")
                    .ok_or_else(|| AssistError::Parse("filter.args.value missing".into()))?,
            )?,
        })),
        "limit" => {
            let n = args
                .get("n")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| AssistError::Parse("limit.args.n must be an integer".into()))?;
            if n < 0 {
                return Err(AssistError::Parse("limit.args.n must be >= 0".into()));
            }
            Ok(Transform::Limit(n as usize))
        }
        "cast" => Ok(Transform::Cast {
            column: str_arg(args, "column")?,
            to: coltype_from_str(&str_arg(args, "to")?)?,
        }),
        "derive" => Ok(Transform::Derive {
            name: str_arg(args, "name")?,
            expr: expr_from_json(
                args.get("expr")
                    .ok_or_else(|| AssistError::Parse("derive.args.expr missing".into()))?,
            )?,
            ty: coltype_from_str(&str_arg(args, "ty")?)?,
        }),
        other => Err(AssistError::Parse(format!(
            "op '{other}' is not in TRANSFORM_LIBRARY"
        ))),
    }
}

fn str_arg(args: &JsonValue, key: &str) -> Result<String, AssistError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AssistError::Parse(format!("missing string arg '{key}'")))
}

fn cmp_from_str(s: &str) -> Result<CmpOp, AssistError> {
    match s {
        "eq" => Ok(CmpOp::Eq),
        "ne" => Ok(CmpOp::Ne),
        "lt" => Ok(CmpOp::Lt),
        "le" => Ok(CmpOp::Le),
        "gt" => Ok(CmpOp::Gt),
        "ge" => Ok(CmpOp::Ge),
        other => Err(AssistError::Parse(format!("unknown comparator '{other}'"))),
    }
}

fn coltype_from_str(s: &str) -> Result<ColumnType, AssistError> {
    match s {
        "int" => Ok(ColumnType::Int),
        "float" => Ok(ColumnType::Float),
        "text" => Ok(ColumnType::Text),
        "bool" => Ok(ColumnType::Bool),
        "bytes" => Ok(ColumnType::Bytes),
        other => Err(AssistError::Parse(format!("unknown column type '{other}'"))),
    }
}

fn binop_from_str(s: &str) -> Result<BinOp, AssistError> {
    match s {
        "add" => Ok(BinOp::Add),
        "sub" => Ok(BinOp::Sub),
        "mul" => Ok(BinOp::Mul),
        "concat" => Ok(BinOp::Concat),
        other => Err(AssistError::Parse(format!("unknown binop '{other}'"))),
    }
}

/// Map a JSON scalar to a typed cell value (a JSON null becomes a null cell).
fn value_from_json(v: &JsonValue) -> Result<DataValue, AssistError> {
    match v {
        JsonValue::Null => Ok(DataValue::Null),
        JsonValue::Bool(b) => Ok(DataValue::Bool(*b)),
        JsonValue::Str(s) => Ok(DataValue::Text(s.clone())),
        JsonValue::Num(n) if n.fract() == 0.0 => Ok(DataValue::Int(*n as i64)),
        JsonValue::Num(n) => Ok(DataValue::Float(*n)),
        _ => Err(AssistError::Parse("value must be a scalar".into())),
    }
}

/// Map a JSON expression (`{col}`, `{lit}`, or `{binop,left,right}`) to an [`Expr`].
fn expr_from_json(v: &JsonValue) -> Result<Expr, AssistError> {
    if let Some(col) = v.get("col").and_then(|c| c.as_str()) {
        return Ok(Expr::Column(col.to_string()));
    }
    if let Some(lit) = v.get("lit") {
        return Ok(Expr::Lit(value_from_json(lit)?));
    }
    if let Some(binop) = v.get("binop").and_then(|b| b.as_str()) {
        let left = v
            .get("left")
            .ok_or_else(|| AssistError::Parse("binop expr missing 'left'".into()))?;
        let right = v
            .get("right")
            .ok_or_else(|| AssistError::Parse("binop expr missing 'right'".into()))?;
        return Ok(Expr::Binary(
            binop_from_str(binop)?,
            Box::new(expr_from_json(left)?),
            Box::new(expr_from_json(right)?),
        ));
    }
    Err(AssistError::Parse(
        "expr must be one of {col}, {lit}, or {binop,left,right}".into(),
    ))
}
