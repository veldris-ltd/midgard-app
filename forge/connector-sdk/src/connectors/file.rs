//! A reference **file** connector: parse delimited text (CSV-class) into a typed
//! dataset against a declared schema. Offline and credential-free — it operates on
//! bytes already inside the enclosure. Typed parsing means a malformed source is
//! rejected at ingest, not discovered three transforms later. [FR-F-2]

use std::path::{Path, PathBuf};

use forge_dataset_store::{ColumnType, DataValue, Dataset, Schema};

use crate::connector::{Connector, ConnectorDescriptor, ConnectorKind, SyncContext};
use crate::error::ConnectorError;

/// Parses delimited text into a dataset of a fixed schema.
pub struct DelimitedConnector {
    source: String,
    schema: Schema,
    body: String,
    delimiter: char,
    has_header: bool,
}

impl DelimitedConnector {
    /// Build a delimited-file connector. `body` is the raw file content already in
    /// the enclosure; `delimiter` separates fields; `has_header` skips row 0.
    pub fn new(
        source: impl Into<String>,
        schema: Schema,
        body: impl Into<String>,
        delimiter: char,
        has_header: bool,
    ) -> Self {
        Self {
            source: source.into(),
            schema,
            body: body.into(),
            delimiter,
            has_header,
        }
    }

    /// A comma-delimited connector with a header row (the common case).
    pub fn csv(source: impl Into<String>, schema: Schema, body: impl Into<String>) -> Self {
        Self::new(source, schema, body, ',', true)
    }

    /// Parse one raw field into a typed cell per the column's type/nullability.
    fn parse_cell(
        raw: &str,
        ty: ColumnType,
        nullable: bool,
        row: usize,
        column: &str,
    ) -> Result<DataValue, ConnectorError> {
        let field = raw.trim();
        if field.is_empty() {
            if nullable {
                return Ok(DataValue::Null);
            }
            return Err(ConnectorError::Parse(format!(
                "row {row}: empty value in non-nullable column '{column}'"
            )));
        }
        let parsed = match ty {
            ColumnType::Int => field
                .parse::<i64>()
                .map(DataValue::Int)
                .map_err(|_| ())
                .ok(),
            // "nan" and "inf" parse as f64 but are not values: NaN has no JSON form and
            // would reach the fabric as null. No value where the column allows none,
            // else refused like any bad number.
            ColumnType::Float => match field.parse::<f64>() {
                Ok(x) if x.is_finite() => Some(DataValue::Float(x)),
                Ok(_) if nullable => return Ok(DataValue::Null),
                _ => None,
            },
            ColumnType::Bool => match field.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => Some(DataValue::Bool(true)),
                "false" | "0" | "no" => Some(DataValue::Bool(false)),
                _ => None,
            },
            ColumnType::Text => Some(DataValue::Text(field.to_string())),
            ColumnType::Bytes => Some(DataValue::Bytes(field.as_bytes().to_vec())),
        };
        parsed.ok_or_else(|| {
            ConnectorError::Parse(format!(
                "row {row}: value '{field}' is not a valid {} for column '{column}'",
                ty.as_str()
            ))
        })
    }

    /// Parse delimited `body` into a dataset against `schema`. Shared by the
    /// in-enclosure [`DelimitedConnector`], the disk-reading [`FileConnector`], and
    /// the API connector's post-fetch parse.
    pub(crate) fn parse_body(
        schema: &Schema,
        body: &str,
        delimiter: char,
        has_header: bool,
    ) -> Result<Dataset, ConnectorError> {
        let width = schema.width();
        let mut rows = Vec::new();
        let records = crate::csv::records(body, delimiter).map_err(ConnectorError::Parse)?;
        for (i, fields) in records.iter().enumerate() {
            if has_header && i == 0 {
                continue;
            }
            if fields.len() == 1 && fields[0].is_empty() {
                continue;
            }
            if fields.len() != width {
                return Err(ConnectorError::Parse(format!(
                    "row {i}: expected {width} fields, found {}",
                    fields.len()
                )));
            }
            let mut cells = Vec::with_capacity(width);
            for (field, col) in fields.iter().zip(&schema.columns) {
                cells.push(Self::parse_cell(field, col.ty, col.nullable, i, &col.name)?);
            }
            rows.push(cells);
        }
        Ok(Dataset::new(schema.clone(), rows)?)
    }
}

impl Connector for DelimitedConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: ConnectorKind::File,
            source: self.source.clone(),
            streaming: false,
        }
    }

    fn sync(&self, _ctx: &SyncContext) -> Result<Dataset, ConnectorError> {
        Self::parse_body(&self.schema, &self.body, self.delimiter, self.has_header)
    }
}

/// A real **file** connector: it opens a delimited file **from disk** at sync time
/// (genuine local I/O, no network) and parses it against a declared schema. This is
/// the reference source for local/mounted files already inside the enclosure — the
/// bytes never leave, and no egress is attempted, so it runs under the air-gapped
/// default sandbox. [FR-F-2] [NFR-PRV-1]
pub struct FileConnector {
    path: PathBuf,
    schema: Schema,
    delimiter: char,
    has_header: bool,
}

impl FileConnector {
    /// Bind a delimited file on disk at `path`.
    pub fn new(
        path: impl Into<PathBuf>,
        schema: Schema,
        delimiter: char,
        has_header: bool,
    ) -> Self {
        Self {
            path: path.into(),
            schema,
            delimiter,
            has_header,
        }
    }

    /// A CSV file (comma-delimited, header row).
    pub fn csv(path: impl Into<PathBuf>, schema: Schema) -> Self {
        Self::new(path, schema, ',', true)
    }

    /// The bound path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Connector for FileConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: ConnectorKind::File,
            source: self.path.display().to_string(),
            streaming: false,
        }
    }

    fn sync(&self, _ctx: &SyncContext) -> Result<Dataset, ConnectorError> {
        // Genuine disk I/O: open and read the file at sync time.
        let body = std::fs::read_to_string(&self.path)
            .map_err(|e| ConnectorError::Io(format!("reading '{}': {e}", self.path.display())))?;
        DelimitedConnector::parse_body(&self.schema, &body, self.delimiter, self.has_header)
    }
}
