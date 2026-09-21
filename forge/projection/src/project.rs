//! The projection driver: turn a curated dataset into object/link instances in the
//! fabric, propagating the mapping's marking onto every object. Fails closed on an
//! unmapped column, an unusable id, or a value with no fabric representation.
//! [FR-F-4] [NFR-SEC-2]

use std::collections::BTreeMap;

use fabric::object::{LinkInstance, ObjectInstance};
use fabric::store::ObjectStore;
use fabric::value::Value;
use forge_dataset_store::{DataValue, Dataset};
use policy::Marking;

use crate::error::ProjectionError;
use crate::mapping::{LinkMapping, ObjectMapping};

/// What a projection produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionReport {
    /// The object/link type projected into.
    pub type_id: String,
    /// Number of instances written.
    pub count: usize,
    /// The marking propagated onto every projected object (objects only).
    pub marking: Option<Marking>,
}

/// Project a curated dataset into object instances of `mapping.type_id`, writing
/// each into `store` with the mapping's marking and purposes propagated. [FR-F-4]
/// [NFR-SEC-2]
pub fn project_objects<S: ObjectStore>(
    dataset: &Dataset,
    mapping: &ObjectMapping,
    store: &mut S,
) -> Result<ProjectionReport, ProjectionError> {
    let schema = dataset.schema();
    let id_idx = schema
        .index_of(&mapping.id_column)
        .ok_or_else(|| ProjectionError::UnknownColumn(mapping.id_column.clone()))?;

    // Resolve property column indices once.
    let prop_idxs: Vec<(usize, &str)> = mapping
        .properties
        .iter()
        .map(|pm| {
            schema
                .index_of(&pm.column)
                .map(|i| (i, pm.property.as_str()))
                .ok_or_else(|| ProjectionError::UnknownColumn(pm.column.clone()))
        })
        .collect::<Result<_, _>>()?;

    let mut count = 0usize;
    for (row_idx, row) in dataset.rows().iter().enumerate() {
        let id = object_id(&row[id_idx]).ok_or_else(|| ProjectionError::InvalidKey {
            column: mapping.id_column.clone(),
            row: row_idx,
        })?;

        let mut properties: BTreeMap<String, Value> = BTreeMap::new();
        for (i, (col_idx, prop)) in prop_idxs.iter().enumerate() {
            match to_fabric_value(&row[*col_idx]) {
                // Nulls are simply omitted (absent property), not written as a value.
                Ok(None) => {}
                Ok(Some(v)) => {
                    properties.insert(prop.to_string(), v);
                }
                Err(()) => {
                    return Err(ProjectionError::UnrepresentableValue {
                        column: mapping.properties[i].column.clone(),
                        row: row_idx,
                    })
                }
            }
        }

        store.put_object(ObjectInstance {
            id,
            type_id: mapping.type_id.clone(),
            properties,
            // Marking propagation: the dataset's classification is carried through.
            marking: mapping.marking.clone(),
            allowed_purposes: mapping.allowed_purposes.clone(),
        });
        count += 1;
    }

    Ok(ProjectionReport {
        type_id: mapping.type_id.clone(),
        count,
        marking: Some(mapping.marking.clone()),
    })
}

/// Project a curated dataset into directed links of `mapping.link_type`. [FR-F-4]
pub fn project_links<S: ObjectStore>(
    dataset: &Dataset,
    mapping: &LinkMapping,
    store: &mut S,
) -> Result<ProjectionReport, ProjectionError> {
    let schema = dataset.schema();
    let src_idx = schema
        .index_of(&mapping.source_id_column)
        .ok_or_else(|| ProjectionError::UnknownColumn(mapping.source_id_column.clone()))?;
    let tgt_idx = schema
        .index_of(&mapping.target_id_column)
        .ok_or_else(|| ProjectionError::UnknownColumn(mapping.target_id_column.clone()))?;

    let mut count = 0usize;
    for (row_idx, row) in dataset.rows().iter().enumerate() {
        let source_id = object_id(&row[src_idx]).ok_or_else(|| ProjectionError::InvalidKey {
            column: mapping.source_id_column.clone(),
            row: row_idx,
        })?;
        let target_id = object_id(&row[tgt_idx]).ok_or_else(|| ProjectionError::InvalidKey {
            column: mapping.target_id_column.clone(),
            row: row_idx,
        })?;
        store.put_link(LinkInstance {
            link_type: mapping.link_type.clone(),
            source_id,
            target_id,
        });
        count += 1;
    }

    Ok(ProjectionReport {
        type_id: mapping.link_type.clone(),
        count,
        marking: None,
    })
}

/// Derive a stable object id from a key cell. Only text and int make stable ids;
/// floats, bools, bytes, and nulls do not.
fn object_id(v: &DataValue) -> Option<String> {
    match v {
        DataValue::Text(s) => Some(s.clone()),
        DataValue::Int(i) => Some(i.to_string()),
        _ => None,
    }
}

/// Convert a dataset value to a fabric value. `Ok(None)` means null (omit the
/// property); `Err(())` means the type has no fabric representation.
fn to_fabric_value(v: &DataValue) -> Result<Option<Value>, ()> {
    match v {
        DataValue::Null => Ok(None),
        DataValue::Int(i) => Ok(Some(Value::Int(*i))),
        DataValue::Float(f) => Ok(Some(Value::Float(*f))),
        DataValue::Text(s) => Ok(Some(Value::Text(s.clone()))),
        DataValue::Bool(b) => Ok(Some(Value::Bool(*b))),
        DataValue::Bytes(_) => Err(()),
    }
}
