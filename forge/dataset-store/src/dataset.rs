//! An in-memory dataset: a schema plus type-checked rows. Construction validates
//! every cell against the schema, so an invalid dataset is unrepresentable — the
//! store only ever seals well-typed data.

use crate::error::DataError;
use crate::schema::Schema;
use crate::value::DataValue;

/// A row is a positional vector of cells aligned to the schema's columns.
pub type Row = Vec<DataValue>;

/// A validated dataset: `rows` are guaranteed to honour `schema`.
#[derive(Debug, Clone, PartialEq)]
pub struct Dataset {
    schema: Schema,
    rows: Vec<Row>,
}

impl Dataset {
    /// Build a dataset, validating each row against the schema (arity, cell type,
    /// nullability). Returns the first violation rather than silently coercing.
    pub fn new(schema: Schema, rows: Vec<Row>) -> Result<Self, DataError> {
        for (r, row) in rows.iter().enumerate() {
            if row.len() != schema.width() {
                return Err(DataError::ArityMismatch {
                    row: r,
                    expected: schema.width(),
                    found: row.len(),
                });
            }
            for (cell, col) in row.iter().zip(&schema.columns) {
                match cell {
                    DataValue::Null if col.nullable => {}
                    DataValue::Null => {
                        return Err(DataError::NullInNonNullable {
                            row: r,
                            column: col.name.clone(),
                        })
                    }
                    v if col.ty.accepts(v) => {}
                    _ => {
                        return Err(DataError::TypeMismatch {
                            row: r,
                            column: col.name.clone(),
                            expected: col.ty.as_str(),
                        })
                    }
                }
            }
        }
        Ok(Self { schema, rows })
    }

    /// An empty dataset with the given schema.
    pub fn empty(schema: Schema) -> Self {
        Self {
            schema,
            rows: Vec::new(),
        }
    }

    /// The schema.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The rows.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Number of rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Consume the dataset into its parts.
    pub fn into_parts(self) -> (Schema, Vec<Row>) {
        (self.schema, self.rows)
    }
}
