//! Error types for dataset construction and the sealed store. Every failure is [FR-F-1]
//! explicit and fails closed — a malformed dataset never reaches the store, and a
//! blob that will not unseal is never returned as data.

/// A dataset-construction error: the rows did not honour the schema contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    /// A row had the wrong number of cells.
    ArityMismatch {
        /// Zero-based row index.
        row: usize,
        /// Columns the schema declares.
        expected: usize,
        /// Cells the row supplied.
        found: usize,
    },
    /// A cell's value did not match its column type.
    TypeMismatch {
        /// Zero-based row index.
        row: usize,
        /// Column name.
        column: String,
        /// Declared column type.
        expected: &'static str,
    },
    /// A `Null` appeared in a non-nullable column.
    NullInNonNullable {
        /// Zero-based row index.
        row: usize,
        /// Column name.
        column: String,
    },
}

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataError::ArityMismatch {
                row,
                expected,
                found,
            } => write!(f, "row {row}: expected {expected} cells, found {found}"),
            DataError::TypeMismatch {
                row,
                column,
                expected,
            } => write!(f, "row {row}: column '{column}' expects {expected}"),
            DataError::NullInNonNullable { row, column } => {
                write!(f, "row {row}: null in non-nullable column '{column}'")
            }
        }
    }
}

impl std::error::Error for DataError {}

/// A store error: sealing/unsealing failed, a version was unknown, or a sealed
/// blob failed to decode back into a dataset.
#[derive(Debug)]
pub enum StoreError {
    /// The KMS refused to seal or unseal (wrong trust root, tampered blob, …).
    Kms(kms::KmsError),
    /// A referenced content id / branch is not in the store.
    UnknownVersion,
    /// A sealed blob unsealed but did not decode into a valid dataset — corruption
    /// or a version-skew bug. Fails closed rather than returning partial data.
    Decode(String),
    /// The durable backing store failed to persist or recover (I/O, or a
    /// malformed/tampered on-disk blob/ref). Fails closed. [RC-11]
    Io(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Kms(e) => write!(f, "kms: {e:?}"),
            StoreError::UnknownVersion => write!(f, "unknown dataset version"),
            StoreError::Decode(m) => write!(f, "dataset decode failed: {m}"),
            StoreError::Io(m) => write!(f, "durable store i/o: {m}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<kms::KmsError> for StoreError {
    fn from(e: kms::KmsError) -> Self {
        StoreError::Kms(e)
    }
}
