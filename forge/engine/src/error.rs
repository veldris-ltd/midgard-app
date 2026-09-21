//! Engine errors. Both schema inference and execution fail closed: a plan that
//! does not type-check never runs, and a runtime type violation aborts the build
//! rather than emitting coerced data.

/// A compile- or run-time transform failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// A transform referenced a column that does not exist in its input schema.
    UnknownColumn(String),
    /// A transform would produce a duplicate column name.
    DuplicateColumn(String),
    /// A type rule was violated (bad arithmetic operands, impossible cast, a
    /// derived value not matching its declared type, …).
    TypeError(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::UnknownColumn(c) => write!(f, "unknown column '{c}'"),
            EngineError::DuplicateColumn(c) => write!(f, "duplicate column '{c}'"),
            EngineError::TypeError(m) => write!(f, "type error: {m}"),
        }
    }
}

impl std::error::Error for EngineError {}
