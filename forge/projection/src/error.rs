//! Projection errors. Mapping a dataset into the object fabric fails closed: an
//! unmapped column, an unusable key, or an unrepresentable value aborts the
//! projection rather than writing partial or mistyped objects.

/// A projection failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    /// A mapping referenced a column absent from the dataset schema.
    UnknownColumn(String),
    /// A row's id cell could not form a stable object id (null, float, or bytes).
    InvalidKey {
        /// The id column.
        column: String,
        /// Zero-based row index.
        row: usize,
    },
    /// A value type has no object-fabric representation (e.g. raw bytes).
    UnrepresentableValue {
        /// The property column.
        column: String,
        /// Zero-based row index.
        row: usize,
    },
}

impl std::fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectionError::UnknownColumn(c) => write!(f, "unknown column '{c}'"),
            ProjectionError::InvalidKey { column, row } => {
                write!(
                    f,
                    "row {row}: column '{column}' cannot serve as an object id"
                )
            }
            ProjectionError::UnrepresentableValue { column, row } => write!(
                f,
                "row {row}: column '{column}' has no object-fabric value type"
            ),
        }
    }
}

impl std::error::Error for ProjectionError {}
