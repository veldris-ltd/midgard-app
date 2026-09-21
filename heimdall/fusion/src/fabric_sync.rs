//! Projection of tracked entities into the Midgard object fabric. Fusion writes the
//! current picture back to the fabric as typed objects, **propagating a marking onto
//! every projected entity** so classification is never lost at the picture→object
//! boundary — the same discipline Forge's projection mapper uses. [FR-H-2]
//! [NFR-SEC-2]

use std::collections::{BTreeMap, BTreeSet};

use fabric::object::ObjectInstance;
use fabric::store::ObjectStore;
use fabric::value::Value;
use policy::{Marking, Purpose};

use heimdall_picture::{Attr, Picture, TrackedEntity};

/// The fabric object type projected tracked entities become.
pub const ENTITY_TYPE_ID: &str = "heimdall.tracked_entity";

/// Project one tracked entity into a fabric object instance, stamping the supplied
/// marking and purposes. Track state and the current classification are carried as
/// properties so the fabric view is self-describing.
pub fn project_entity(
    entity: &TrackedEntity,
    marking: Marking,
    allowed_purposes: BTreeSet<Purpose>,
) -> ObjectInstance {
    let mut properties: BTreeMap<String, Value> = BTreeMap::new();
    properties.insert("entity_key".into(), Value::Text(entity.entity_key.clone()));
    properties.insert("kind".into(), Value::Text(entity.kind.clone()));
    properties.insert("layer".into(), Value::Text(entity.layer.0.clone()));
    properties.insert(
        "sources".into(),
        Value::Text(entity.sources.iter().cloned().collect::<Vec<_>>().join(",")),
    );
    properties.insert(
        "first_seen".into(),
        Value::Int(entity.track.first_seen as i64),
    );
    properties.insert(
        "last_seen".into(),
        Value::Int(entity.track.last_seen as i64),
    );
    properties.insert(
        "observation_count".into(),
        Value::Int(entity.track.observation_count as i64),
    );
    if let Some(c) = &entity.classification {
        properties.insert("classification".into(), Value::Text(c.label.clone()));
        properties.insert(
            "classification_rationale".into(),
            Value::Text(c.rationale.clone()),
        );
    }
    // Carry the latest observed attributes, namespaced to avoid clashes.
    for (k, v) in &entity.latest_attrs {
        properties.insert(format!("attr.{k}"), attr_to_value(v));
    }

    ObjectInstance {
        id: entity.id.0.clone(),
        type_id: ENTITY_TYPE_ID.to_string(),
        properties,
        marking,
        allowed_purposes,
    }
}

/// Project every entity in a picture into `store`, returning the number written.
/// The same marking and purposes are applied to all — callers project per-marking
/// slices when entities carry differing sensitivities. [FR-H-2] [NFR-SEC-2]
pub fn sync_to_fabric<S: ObjectStore>(
    picture: &Picture,
    store: &mut S,
    marking: &Marking,
    allowed_purposes: &BTreeSet<Purpose>,
) -> usize {
    let mut count = 0;
    for entity in picture.entities() {
        store.put_object(project_entity(
            entity,
            marking.clone(),
            allowed_purposes.clone(),
        ));
        count += 1;
    }
    count
}

/// Map a picture attribute to a fabric value.
fn attr_to_value(a: &Attr) -> Value {
    match a {
        Attr::Text(s) => Value::Text(s.clone()),
        Attr::Int(i) => Value::Int(*i),
        Attr::Float(f) => Value::Float(*f),
        Attr::Bool(b) => Value::Bool(*b),
    }
}
