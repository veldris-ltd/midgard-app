//! The canonical dataset codec: a deterministic, self-describing byte encoding of
//! a [`Dataset`]. It is the single source of truth for **content addressing** (its
//! SHA-256 is the dataset's content id) and for **at-rest sealing** (these bytes
//! are what the KMS seals). Encode is total; decode fails closed on any malformed
//! input. Same dataset → same bytes → same content id, on any host, offline.
//! [FR-F-3] [NFR-PQ-2] [NFR-PRV-1]

use crate::dataset::{Dataset, Row};
use crate::error::StoreError;
use crate::schema::{Column, ColumnType, Schema};
use crate::value::{DataValue, TAG_BOOL, TAG_BYTES, TAG_FLOAT, TAG_INT, TAG_NULL, TAG_TEXT};

/// Format version — bump only on a breaking encoding change.
const FORMAT_VERSION: u8 = 0x01;

/// Encode a dataset to its canonical bytes.
pub(crate) fn encode(dataset: &Dataset) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(FORMAT_VERSION);
    dataset.schema().encode(&mut out);
    out.extend_from_slice(&(dataset.row_count() as u64).to_le_bytes());
    for row in dataset.rows() {
        for cell in row {
            cell.encode(&mut out);
        }
    }
    out
}

/// A bounds-checked reader over canonical bytes.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn u8(&mut self) -> Result<u8, StoreError> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| StoreError::Decode("unexpected end of buffer".into()))?;
        self.pos += 1;
        Ok(b)
    }

    fn u64(&mut self) -> Result<u64, StoreError> {
        let end = self.pos + 8;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| StoreError::Decode("truncated u64".into()))?;
        let mut a = [0u8; 8];
        a.copy_from_slice(slice);
        self.pos = end;
        Ok(u64::from_le_bytes(a))
    }

    fn i64(&mut self) -> Result<i64, StoreError> {
        Ok(self.u64()? as i64)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], StoreError> {
        let end = self.pos + n;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| StoreError::Decode("truncated byte run".into()))?;
        self.pos = end;
        Ok(slice)
    }

    /// A length-prefixed (u64) byte run.
    fn lp_bytes(&mut self) -> Result<&'a [u8], StoreError> {
        let n = self.u64()? as usize;
        self.bytes(n)
    }

    fn value(&mut self) -> Result<DataValue, StoreError> {
        let tag = self.u8()?;
        match tag {
            TAG_NULL => Ok(DataValue::Null),
            TAG_BOOL => Ok(DataValue::Bool(self.u8()? != 0)),
            TAG_INT => Ok(DataValue::Int(self.i64()?)),
            TAG_FLOAT => Ok(DataValue::Float(f64::from_bits(self.u64()?))),
            TAG_TEXT => {
                let raw = self.lp_bytes()?;
                let s = std::str::from_utf8(raw)
                    .map_err(|_| StoreError::Decode("invalid utf-8 in text cell".into()))?;
                Ok(DataValue::Text(s.to_string()))
            }
            TAG_BYTES => Ok(DataValue::Bytes(self.lp_bytes()?.to_vec())),
            other => Err(StoreError::Decode(format!("unknown value tag {other}"))),
        }
    }

    fn schema(&mut self) -> Result<Schema, StoreError> {
        let n = self.u64()? as usize;
        let mut columns = Vec::with_capacity(n);
        for _ in 0..n {
            let ty = ColumnType::from_tag(self.u8()?)
                .ok_or_else(|| StoreError::Decode("unknown column type tag".into()))?;
            let nullable = self.u8()? != 0;
            let name_raw = self.lp_bytes()?;
            let name = std::str::from_utf8(name_raw)
                .map_err(|_| StoreError::Decode("invalid utf-8 in column name".into()))?
                .to_string();
            columns.push(Column { name, ty, nullable });
        }
        Ok(Schema::new(columns))
    }
}

/// Decode canonical bytes back into a validated dataset. Re-runs schema validation
/// (via [`Dataset::new`]) so a corrupted blob can never surface as a "valid"
/// dataset.
pub(crate) fn decode(buf: &[u8]) -> Result<Dataset, StoreError> {
    let mut r = Reader::new(buf);
    let version = r.u8()?;
    if version != FORMAT_VERSION {
        return Err(StoreError::Decode(format!(
            "unsupported dataset format version {version}"
        )));
    }
    let schema = r.schema()?;
    let width = schema.width();
    let row_count = r.u64()? as usize;
    let mut rows: Vec<Row> = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        let mut row = Vec::with_capacity(width);
        for _ in 0..width {
            row.push(r.value()?);
        }
        rows.push(row);
    }
    Dataset::new(schema, rows).map_err(|e| StoreError::Decode(e.to_string()))
}
