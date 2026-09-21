//! Dataset schema: the typed column contract that the low-code builder and the
//! pro-code IDE share (FR-F-1 authors against *this*, never a private shape). The
//! schema is canonically encodable so a schema change is itself a content change.

use crate::value::DataValue;

/// The type of a column's cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    /// 64-bit signed integer.
    Int,
    /// 64-bit float.
    Float,
    /// UTF-8 text.
    Text,
    /// Boolean.
    Bool,
    /// Opaque bytes.
    Bytes,
}

impl ColumnType {
    /// Stable canonical discriminant.
    pub(crate) fn tag(self) -> u8 {
        match self {
            ColumnType::Int => 1,
            ColumnType::Float => 2,
            ColumnType::Text => 3,
            ColumnType::Bool => 4,
            ColumnType::Bytes => 5,
        }
    }

    /// Inverse of [`ColumnType::tag`], for canonical decoding.
    pub(crate) fn from_tag(tag: u8) -> Option<ColumnType> {
        match tag {
            1 => Some(ColumnType::Int),
            2 => Some(ColumnType::Float),
            3 => Some(ColumnType::Text),
            4 => Some(ColumnType::Bool),
            5 => Some(ColumnType::Bytes),
            _ => None,
        }
    }

    /// The contract string (used in type-error messages and previews).
    pub fn as_str(self) -> &'static str {
        match self {
            ColumnType::Int => "int",
            ColumnType::Float => "float",
            ColumnType::Text => "text",
            ColumnType::Bool => "bool",
            ColumnType::Bytes => "bytes",
        }
    }

    /// True iff `value` inhabits this column type (ignoring nullability).
    pub fn accepts(self, value: &DataValue) -> bool {
        matches!(
            (self, value),
            (ColumnType::Int, DataValue::Int(_))
                | (ColumnType::Float, DataValue::Float(_))
                | (ColumnType::Text, DataValue::Text(_))
                | (ColumnType::Bool, DataValue::Bool(_))
                | (ColumnType::Bytes, DataValue::Bytes(_))
        )
    }
}

/// One named, typed column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// Column name (unique within a schema).
    pub name: String,
    /// Cell type.
    pub ty: ColumnType,
    /// Whether `Null` is a legal cell.
    pub nullable: bool,
}

impl Column {
    /// A non-nullable column.
    pub fn new(name: impl Into<String>, ty: ColumnType) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable: false,
        }
    }

    /// A nullable column.
    pub fn nullable(name: impl Into<String>, ty: ColumnType) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable: true,
        }
    }
}

/// An ordered set of typed columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    /// Columns, in positional order.
    pub columns: Vec<Column>,
}

impl Schema {
    /// Build a schema from columns.
    pub fn new(columns: Vec<Column>) -> Self {
        Self { columns }
    }

    /// Number of columns.
    pub fn width(&self) -> usize {
        self.columns.len()
    }

    /// Position of a column by name.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    /// A column by name.
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Append the schema's canonical encoding to `out`: a column count then, per
    /// column, its type tag, nullability, and length-prefixed name.
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&(self.columns.len() as u64).to_le_bytes());
        for c in &self.columns {
            out.push(c.ty.tag());
            out.push(c.nullable as u8);
            out.extend_from_slice(&(c.name.len() as u64).to_le_bytes());
            out.extend_from_slice(c.name.as_bytes());
        }
    }
}
