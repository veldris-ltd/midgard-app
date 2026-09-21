//! Projection mappings: how a curated dataset's columns become object/link types
//! in the Midgard fabric, and the marking/purposes propagated onto every projected
//! object. The marking on the mapping *is* the dataset's classification carried
//! into the object layer — projection never launders a marking away. [FR-F-4]
//! [NFR-SEC-2]

use std::collections::BTreeSet;

use policy::{Marking, Purpose};

/// Maps one dataset column to one object property api-name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyMapping {
    /// Source dataset column.
    pub column: String,
    /// Target object property name.
    pub property: String,
}

impl PropertyMapping {
    /// A property mapping.
    pub fn new(column: impl Into<String>, property: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            property: property.into(),
        }
    }
}

/// Maps a curated dataset into object instances of one object type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMapping {
    /// Target object type id.
    pub type_id: String,
    /// Column whose value is each object's primary key.
    pub id_column: String,
    /// Column → property mappings.
    pub properties: Vec<PropertyMapping>,
    /// The marking stamped on every projected object (propagated from the dataset's
    /// classification). [NFR-SEC-2]
    pub marking: Marking,
    /// Purposes each projected object may be accessed for.
    pub allowed_purposes: BTreeSet<Purpose>,
}

impl ObjectMapping {
    /// Build an object mapping.
    pub fn new(
        type_id: impl Into<String>,
        id_column: impl Into<String>,
        properties: Vec<PropertyMapping>,
        marking: Marking,
        allowed_purposes: BTreeSet<Purpose>,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            id_column: id_column.into(),
            properties,
            marking,
            allowed_purposes,
        }
    }
}

/// Maps a curated dataset into directed links between two id columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkMapping {
    /// Target link type id.
    pub link_type: String,
    /// Column holding the source object id.
    pub source_id_column: String,
    /// Column holding the target object id.
    pub target_id_column: String,
}

impl LinkMapping {
    /// Build a link mapping.
    pub fn new(
        link_type: impl Into<String>,
        source_id_column: impl Into<String>,
        target_id_column: impl Into<String>,
    ) -> Self {
        Self {
            link_type: link_type.into(),
            source_id_column: source_id_column.into(),
            target_id_column: target_id_column.into(),
        }
    }
}
