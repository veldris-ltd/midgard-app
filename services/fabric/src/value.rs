//! Typed property values and their value types. The object fabric is typed end to
//! end: every property has a declared [`ValueType`], and every stored [`Value`]
//! must match it. Filters arrive as strings (the proto surface is
//! `map<string,string>`) and are parsed against the schema's value type, so a
//! filter can never smuggle an ill-typed comparison. [DD-1]

/// The declared type of a property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    /// UTF-8 text.
    Text,
    /// 64-bit signed integer.
    Int,
    /// 64-bit float.
    Float,
    /// Boolean.
    Bool,
    /// Unix-seconds timestamp.
    Timestamp,
}

impl ValueType {
    /// Stable wire/name string.
    pub fn as_str(self) -> &'static str {
        match self {
            ValueType::Text => "text",
            ValueType::Int => "int",
            ValueType::Float => "float",
            ValueType::Bool => "bool",
            ValueType::Timestamp => "timestamp",
        }
    }
}

/// A concrete, typed property value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Text.
    Text(String),
    /// Integer.
    Int(i64),
    /// Float.
    Float(f64),
    /// Boolean.
    Bool(bool),
    /// Timestamp (Unix seconds).
    Timestamp(u64),
}

impl Value {
    /// The value's type.
    pub fn value_type(&self) -> ValueType {
        match self {
            Value::Text(_) => ValueType::Text,
            Value::Int(_) => ValueType::Int,
            Value::Float(_) => ValueType::Float,
            Value::Bool(_) => ValueType::Bool,
            Value::Timestamp(_) => ValueType::Timestamp,
        }
    }

    /// A canonical string form used for equality filtering and inverted indexing.
    /// Deterministic so the same value always indexes under the same key.
    pub fn as_index_key(&self) -> String {
        match self {
            Value::Text(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Float(x) => format!("{x:?}"),
            Value::Bool(b) => b.to_string(),
            Value::Timestamp(t) => t.to_string(),
        }
    }

    /// Parse a filter string into a value of the given type. Returns `None` if the
    /// string does not parse as that type (fail closed — no lossy coercion).
    pub fn parse(vt: ValueType, s: &str) -> Option<Value> {
        match vt {
            ValueType::Text => Some(Value::Text(s.to_string())),
            ValueType::Int => s.parse().ok().map(Value::Int),
            ValueType::Float => s.parse().ok().map(Value::Float),
            ValueType::Bool => s.parse().ok().map(Value::Bool),
            ValueType::Timestamp => s.parse().ok().map(Value::Timestamp),
        }
    }
}
