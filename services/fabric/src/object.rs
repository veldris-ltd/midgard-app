//! Object and link *instances* — the data that populates the schema. Every object
//! carries a mandatory [`Marking`] and the set of purposes it may be accessed
//! for, which the query layer feeds to the policy point for dynamic security.
//! [NFR-SEC-2]

use std::collections::{BTreeMap, BTreeSet};

use policy::{Marking, Purpose};

use crate::value::Value;

/// One instance of an object type.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectInstance {
    /// Primary key.
    pub id: String,
    /// The object type id.
    pub type_id: String,
    /// Typed property values, keyed by property api name.
    pub properties: BTreeMap<String, Value>,
    /// Mandatory classification marking.
    pub marking: Marking,
    /// Purposes under which this object may be read.
    pub allowed_purposes: BTreeSet<Purpose>,
}

/// One instance of a link type: a directed edge between two objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkInstance {
    /// The link type id.
    pub link_type: String,
    /// Source object id.
    pub source_id: String,
    /// Target object id.
    pub target_id: String,
}
