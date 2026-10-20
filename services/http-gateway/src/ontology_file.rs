//! The ontology file: a deployment's object types, link types and actions, loaded at
//! startup instead of compiled in. The type registry is not journalled, so a gateway
//! serving a state directory registers this file on every boot, and the objects already
//! in its durable store are typed by it. [DD-1] [NFR-AI-1]
//!
//! ```json
//! {
//!   "version": 1,
//!   "objectTypes": [
//!     { "id": "aircraft", "title": "Aircraft",
//!       "properties": [ { "name": "tail", "type": "text", "required": true },
//!                      { "name": "status", "type": "text" } ] },
//!     { "id": "facility", "title": "Facility",
//!       "properties": [ { "name": "name", "type": "text", "required": true },
//!                      { "name": "seen", "type": "timestamp" } ],
//!       "retention": { "after": ["seen"], "hours": 24 } }
//!   ],
//!   "linkTypes": [
//!     { "id": "aircraft_base", "name": "based at", "from": "aircraft", "to": "facility",
//!       "cardinality": "one-to-one" }
//!   ],
//!   "actions": [
//!     { "id": "ground_aircraft", "name": "groundAircraft", "title": "Ground aircraft",
//!       "objectType": "aircraft", "humanGated": true,
//!       "marking": "confidential", "compartments": ["OPS-N"], "purposes": ["operations"],
//!       "effect": { "kind": "set-property", "property": "status", "value": "grounded" } }
//!   ]
//! }
//! ```
//!
//! - Ids and api names are letters, digits, `_` and `-`: object ids are written
//!   `<type>/<key>`, so a type id may not contain `/`.
//! - Property types are `text`, `int`, `float`, `bool` and `timestamp`; `required`
//!   defaults to false.
//! - Cardinality is `one-to-one`, `one-to-many` or `many-to-many`, and both ends of a
//!   link must be declared object types.
//! - An action's `name` is its api name: the action a policy rule must grant before
//!   anyone can run it. `marking` is a band — `open`, `internal`, `restricted`,
//!   `confidential` — with optional `compartments` a caller must also hold.
//!   `humanGated` defaults to **true**: an action runs without a named human's
//!   authorisation only when the file says so.
//! - `retention` (optional) is how long the type's objects are kept: `hours` after
//!   the latest of the `after` timestamp properties the type declares. The enclosure
//!   removes older objects and audits each sweep; an object holding none of them is
//!   kept. A type without it is kept indefinitely. [SAD_00 §8.5]
//! - Effects: `set-property` sets a text (or true/false) property of every target to
//!   `value`;
//!   `cycle-property` advances a text property to the next of `values` (an object
//!   without it counts as at the first, so the first run sets the second). Either must
//!   name a text or bool property the action's object type declares; for a bool the
//!   values are `"true"` and `"false"`.
//!
//! Loading is strict in the same way as the policy file. Unknown or repeated keys,
//! missing or blank fields, wrong types, unknown values, duplicate ids, and a link or
//! action naming an undeclared type or property all refuse the whole file.

use std::collections::BTreeSet;

use fabric::{Cardinality, LinkType, ObjectType, PropertyType, Retention, ValueType};
use policy::{Compartment, Marking, Purpose};

use crate::enclosure::level_of;
use crate::json::{parse, Json};
use crate::strict_json::{
    array, identifier, object, only_keys, optional_array, optional_bool, strings, text, unique,
};

/// The one ontology file format version this build reads.
pub const ONTOLOGY_FILE_VERSION: i64 = 1;

// The keys each object in the file may carry. They are public so that the published
// schema (`contracts/schema/ontology.v1.schema.json`) can be checked against them: a
// key added here and not there, or the reverse, fails
// `tests/contract_schemas.rs`. [AC-RF-CA-13c]

/// Keys of the file itself.
pub const ROOT_KEYS: &[&str] = &["version", "objectTypes", "linkTypes", "actions"];
/// Keys of an object type.
pub const OBJECT_TYPE_KEYS: &[&str] = &["id", "title", "properties", "retention"];
/// Keys of an object type's retention window.
pub const RETENTION_KEYS: &[&str] = &["after", "hours"];
/// Keys of a property.
pub const PROPERTY_KEYS: &[&str] = &["name", "type", "required"];
/// Keys of a link type.
pub const LINK_TYPE_KEYS: &[&str] = &["id", "name", "from", "to", "cardinality"];
/// Keys of an action.
pub const ACTION_KEYS: &[&str] = &[
    "id",
    "name",
    "title",
    "objectType",
    "humanGated",
    "marking",
    "compartments",
    "purposes",
    "effect",
];
/// Keys of a `set-property` effect.
pub const SET_PROPERTY_KEYS: &[&str] = &["kind", "property", "value"];
/// Keys of a `cycle-property` effect.
pub const CYCLE_PROPERTY_KEYS: &[&str] = &["kind", "property", "values"];

/// A parsed ontology, ready to register.
#[derive(Debug, Clone, PartialEq)]
pub struct Ontology {
    /// Object types, in file order.
    pub object_types: Vec<ObjectType>,
    /// Link types, in file order.
    pub link_types: Vec<LinkType>,
    /// Actions, in file order.
    pub actions: Vec<ActionDecl>,
    /// Retention windows, one per object type that declares one, in file order.
    pub retention: Vec<Retention>,
}

/// An action as the ontology file declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDecl {
    /// Fabric action type id.
    pub id: String,
    /// Api name — the action a policy rule grants.
    pub api_name: String,
    /// The display name the console lists.
    pub title: String,
    /// The object type it acts on.
    pub object_type: String,
    /// Whether a named human must authorise each run.
    pub human_gated: bool,
    /// The marking a caller must dominate to run it.
    pub marking: Marking,
    /// The purposes it may be run under.
    pub purposes: BTreeSet<Purpose>,
    /// What running it does to each target.
    pub effect: ActionEffect,
}

/// What an action does to each object it targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionEffect {
    /// Set a text property to a fixed value.
    SetProperty {
        /// The property.
        property: String,
        /// The value it is set to.
        value: String,
    },
    /// Advance a text property to the next value in a cycle.
    CycleProperty {
        /// The property.
        property: String,
        /// The cycle, in order; a value outside it restarts at the first, and a missing
        /// one counts as the first (so the first run sets the second).
        values: Vec<String>,
    },
}

/// Parse an ontology file.
pub fn parse_ontology(text: &str) -> Result<Ontology, String> {
    parse_document(text).map_err(|e| format!("ontology file refused: {e}"))
}

fn parse_document(text: &str) -> Result<Ontology, String> {
    let doc = parse(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let here = "the ontology file";
    only_keys(object(&doc, here)?, here, ROOT_KEYS)?;
    match doc.get("version") {
        Some(Json::Int(v)) if *v == ONTOLOGY_FILE_VERSION => {}
        Some(Json::Num(v)) if *v == ONTOLOGY_FILE_VERSION as f64 => {}
        Some(other) => {
            return Err(format!(
                "unsupported version {} (this build reads version {ONTOLOGY_FILE_VERSION})",
                other.encode()
            ))
        }
        None => return Err("missing \"version\"".into()),
    }

    let (object_types, retention): (Vec<_>, Vec<_>) = array(&doc, here, "objectTypes")?
        .iter()
        .enumerate()
        .map(|(i, item)| object_type(i, item))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .unzip();
    let retention = retention.into_iter().flatten().collect();
    unique(object_types.iter().map(|t| t.id.as_str()), "object type id")?;

    let link_types = optional_array(&doc, here, "linkTypes")?
        .iter()
        .enumerate()
        .map(|(i, item)| link_type(i, item, &object_types))
        .collect::<Result<Vec<_>, _>>()?;
    unique(link_types.iter().map(|l| l.id.as_str()), "link type id")?;

    let actions = optional_array(&doc, here, "actions")?
        .iter()
        .enumerate()
        .map(|(i, item)| action(i, item, &object_types))
        .collect::<Result<Vec<_>, _>>()?;
    unique(actions.iter().map(|a| a.id.as_str()), "action id")?;
    unique(actions.iter().map(|a| a.api_name.as_str()), "action name")?;

    Ok(Ontology {
        object_types,
        link_types,
        actions,
        retention,
    })
}

fn object_type(index: usize, item: &Json) -> Result<(ObjectType, Option<Retention>), String> {
    let at = format!("objectTypes[{index}]");
    only_keys(object(item, &at)?, &at, OBJECT_TYPE_KEYS)?;
    let id = identifier(item, &at, "id")?;
    let at = format!("object type \"{id}\"");
    let properties = array(item, &at, "properties")?
        .iter()
        .enumerate()
        .map(|(i, p)| property(&at, i, p))
        .collect::<Result<Vec<_>, _>>()?;
    unique(
        properties.iter().map(|p| p.api_name.as_str()),
        &format!("{at}: property name"),
    )?;
    let retention = match item.get("retention") {
        None => None,
        Some(window) => Some(retention(&at, id, window, &properties)?),
    };
    Ok((
        ObjectType {
            id: id.to_string(),
            api_name: id.to_string(),
            title: text(item, &at, "title")?.to_string(),
            properties,
            interfaces: Vec::new(),
        },
        retention,
    ))
}

fn retention(
    owner: &str,
    type_id: &str,
    item: &Json,
    properties: &[PropertyType],
) -> Result<Retention, String> {
    let at = format!("{owner}: retention");
    only_keys(object(item, &at)?, &at, RETENTION_KEYS)?;
    let after = strings(item, &at, "after")?;
    if after.is_empty() {
        return Err(format!("{at}: \"after\" must name at least one property"));
    }
    unique(
        after.iter().map(String::as_str),
        &format!("{at}: \"after\" property"),
    )?;
    for name in &after {
        match properties.iter().find(|p| &p.api_name == name) {
            Some(p) if p.value_type == ValueType::Timestamp => {}
            Some(_) => {
                return Err(format!(
                    "{at}: \"after\" names \"{name}\", which is not a timestamp property"
                ))
            }
            None => {
                return Err(format!(
                    "{at}: \"after\" names \"{name}\", which the type does not declare"
                ))
            }
        }
    }
    let hours = match item.get("hours") {
        Some(Json::Int(h)) if *h >= 1 => *h as u64,
        Some(Json::Num(h)) if h.fract() == 0.0 && *h >= 1.0 => *h as u64,
        Some(other) => {
            return Err(format!(
                "{at}: \"hours\" must be a whole number of at least 1, not {}",
                other.encode()
            ))
        }
        None => return Err(format!("{at}: missing \"hours\"")),
    };
    Ok(Retention {
        object_type: type_id.to_string(),
        after,
        seconds: hours.saturating_mul(3600),
    })
}

fn property(owner: &str, index: usize, item: &Json) -> Result<PropertyType, String> {
    let at = format!("{owner}: properties[{index}]");
    only_keys(object(item, &at)?, &at, PROPERTY_KEYS)?;
    let name = identifier(item, &at, "name")?;
    let at = format!("{owner}: property \"{name}\"");
    let value_type = match text(item, &at, "type")? {
        "text" => ValueType::Text,
        "int" => ValueType::Int,
        "float" => ValueType::Float,
        "bool" => ValueType::Bool,
        "timestamp" => ValueType::Timestamp,
        other => {
            return Err(format!(
                "{at}: \"type\" must be text, int, float, bool or timestamp, not \"{other}\""
            ))
        }
    };
    let required = optional_bool(item, &at, "required", false)?;
    Ok(PropertyType::new(name, value_type, required))
}

fn link_type(index: usize, item: &Json, object_types: &[ObjectType]) -> Result<LinkType, String> {
    let at = format!("linkTypes[{index}]");
    only_keys(object(item, &at)?, &at, LINK_TYPE_KEYS)?;
    let id = identifier(item, &at, "id")?;
    let at = format!("link type \"{id}\"");
    let end = |key: &str| -> Result<String, String> {
        let ty = identifier(item, &at, key)?;
        declared(object_types, ty)
            .map(|t| t.id.clone())
            .ok_or_else(|| {
                format!("{at}: \"{key}\" names \"{ty}\", which is not a declared object type")
            })
    };
    let source_type = end("from")?;
    let target_type = end("to")?;
    let cardinality = match text(item, &at, "cardinality")? {
        "one-to-one" => Cardinality::OneToOne,
        "one-to-many" => Cardinality::OneToMany,
        "many-to-many" => Cardinality::ManyToMany,
        other => {
            return Err(format!(
                "{at}: \"cardinality\" must be one-to-one, one-to-many or many-to-many, not \"{other}\""
            ))
        }
    };
    Ok(LinkType {
        id: id.to_string(),
        api_name: text(item, &at, "name")?.to_string(),
        source_type,
        target_type,
        cardinality,
    })
}

fn action(index: usize, item: &Json, object_types: &[ObjectType]) -> Result<ActionDecl, String> {
    let at = format!("actions[{index}]");
    only_keys(object(item, &at)?, &at, ACTION_KEYS)?;
    let id = identifier(item, &at, "id")?;
    let at = format!("action \"{id}\"");

    let type_id = identifier(item, &at, "objectType")?;
    let target = declared(object_types, type_id).ok_or_else(|| {
        format!("{at}: \"objectType\" names \"{type_id}\", which is not a declared object type")
    })?;

    let band = text(item, &at, "marking")?;
    let level = level_of(band).ok_or_else(|| {
        format!(
            "{at}: \"marking\" must be open, internal, restricted or confidential, not \"{band}\""
        )
    })?;
    let compartments = match item.get("compartments") {
        None => BTreeSet::new(),
        Some(_) => strings(item, &at, "compartments")?
            .into_iter()
            .map(Compartment)
            .collect(),
    };
    let purposes: BTreeSet<Purpose> = strings(item, &at, "purposes")?
        .into_iter()
        .map(Purpose)
        .collect();
    if purposes.is_empty() {
        return Err(format!(
            "{at}: \"purposes\" needs at least one purpose, or the action can never run"
        ));
    }

    Ok(ActionDecl {
        id: id.to_string(),
        api_name: identifier(item, &at, "name")?.to_string(),
        title: text(item, &at, "title")?.to_string(),
        object_type: target.id.clone(),
        human_gated: optional_bool(item, &at, "humanGated", true)?,
        marking: Marking {
            level,
            compartments,
        },
        purposes,
        effect: effect(&at, item.get("effect"), target)?,
    })
}

fn effect(owner: &str, item: Option<&Json>, target: &ObjectType) -> Result<ActionEffect, String> {
    let at = format!("{owner}: effect");
    let item = item.ok_or_else(|| format!("{owner}: missing \"effect\""))?;
    object(item, &at)?;
    // The property an effect sets, and whether it is true/false (else text).
    let text_property = |key: &str| -> Result<(String, bool), String> {
        let name = identifier(item, &at, key)?;
        match target.property(name) {
            Some(p) if p.value_type == ValueType::Text => Ok((name.to_string(), false)),
            Some(p) if p.value_type == ValueType::Bool => Ok((name.to_string(), true)),
            Some(_) => Err(format!(
                "{at}: \"{name}\" is not a text or bool property of \"{}\"",
                target.id
            )),
            None => Err(format!(
                "{at}: \"{}\" declares no property \"{name}\"",
                target.id
            )),
        }
    };
    match text(item, &at, "kind")? {
        "set-property" => {
            only_keys(object(item, &at)?, &at, SET_PROPERTY_KEYS)?;
            let (property, boolean) = text_property("property")?;
            let value = text(item, &at, "value")?.to_string();
            if boolean && value != "true" && value != "false" {
                return Err(format!(
                    "{at}: \"{property}\" is true/false, so \"value\" must be \"true\" or \"false\""
                ));
            }
            Ok(ActionEffect::SetProperty { property, value })
        }
        "cycle-property" => {
            only_keys(object(item, &at)?, &at, CYCLE_PROPERTY_KEYS)?;
            let values = strings(item, &at, "values")?;
            if values.len() < 2 {
                return Err(format!(
                    "{at}: \"values\" needs at least two values to cycle"
                ));
            }
            unique(values.iter().map(String::as_str), &format!("{at}: value"))?;
            let (property, boolean) = text_property("property")?;
            if boolean && values.iter().any(|v| v != "true" && v != "false") {
                return Err(format!(
                    "{at}: \"{property}\" is true/false, so \"values\" must be \"true\" and \"false\""
                ));
            }
            Ok(ActionEffect::CycleProperty { property, values })
        }
        other => Err(format!(
            "{at}: \"kind\" must be set-property or cycle-property, not \"{other}\""
        )),
    }
}

// ---- lookups ---------------------------------------------------------------------------

fn declared<'a>(object_types: &'a [ObjectType], id: &str) -> Option<&'a ObjectType> {
    object_types.iter().find(|t| t.id == id)
}
