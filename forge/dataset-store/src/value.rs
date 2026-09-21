//! Typed cell values that populate a dataset, with a **canonical, dependency-free
//! byte encoding**. Content addressing and reproducible rebuilds both hash this
//! encoding, so it must be total and order-stable: the same logical value always
//! encodes to the same bytes, on any machine, offline. [FR-F-3] [NFR-PRV-1]

/// A single typed cell value. Floats are encoded by their IEEE-754 bit pattern so
/// the encoding is exact and reproducible (no locale/format drift).
#[derive(Debug, Clone, PartialEq)]
pub enum DataValue {
    /// Absent value (only legal in a nullable column).
    Null,
    /// 64-bit signed integer.
    Int(i64),
    /// 64-bit IEEE-754 float.
    Float(f64),
    /// UTF-8 text.
    Text(String),
    /// Boolean.
    Bool(bool),
    /// Opaque bytes.
    Bytes(Vec<u8>),
}

// Canonical tag bytes — stable wire discriminants; never reorder or reuse.
pub(crate) const TAG_NULL: u8 = 0x00;
pub(crate) const TAG_BOOL: u8 = 0x01;
pub(crate) const TAG_INT: u8 = 0x02;
pub(crate) const TAG_FLOAT: u8 = 0x03;
pub(crate) const TAG_TEXT: u8 = 0x04;
pub(crate) const TAG_BYTES: u8 = 0x05;

impl DataValue {
    /// Append this value's canonical encoding to `out`.
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        match self {
            DataValue::Null => out.push(TAG_NULL),
            DataValue::Bool(b) => {
                out.push(TAG_BOOL);
                out.push(*b as u8);
            }
            DataValue::Int(i) => {
                out.push(TAG_INT);
                out.extend_from_slice(&i.to_le_bytes());
            }
            DataValue::Float(f) => {
                out.push(TAG_FLOAT);
                out.extend_from_slice(&f.to_bits().to_le_bytes());
            }
            DataValue::Text(s) => {
                out.push(TAG_TEXT);
                out.extend_from_slice(&(s.len() as u64).to_le_bytes());
                out.extend_from_slice(s.as_bytes());
            }
            DataValue::Bytes(b) => {
                out.push(TAG_BYTES);
                out.extend_from_slice(&(b.len() as u64).to_le_bytes());
                out.extend_from_slice(b);
            }
        }
    }
}
