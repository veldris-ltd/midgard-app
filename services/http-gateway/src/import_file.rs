//! Importing data into a real enclosure: objects and links as JSON Lines, checked
//! against the enclosure's ontology — and then against its store — before anything is
//! written. [DD-1] [RC-10]
//!
//! One record per line; blank lines are skipped.
//!
//! ```text
//! {"object":"aircraft","key":"AC-2214","marking":"confidential","compartments":["OPS-N"],"purposes":["operations"],"properties":{"tail":"MG-404","status":"active","flight_hours":6412}}
//! {"object":"facility","key":"FAC-NORTH","marking":"restricted","purposes":["operations"],"properties":{"name":"Northern operating base"}}
//! {"link":"aircraft_base","from":"AC-2214","to":"FAC-NORTH"}
//! ```
//!
//! - An object's id is `<object type>/<key>`. Every property must be declared by its
//!   type, with a matching JSON value: a string for `text`, an integer for `int`, a
//!   non-negative integer (Unix seconds) for `timestamp`, any number for `float`, and
//!   `true` or `false` for `bool`. Every `required` property must be present.
//! - `marking` is a band — `open`, `internal`, `restricted`, `confidential` — with
//!   optional `compartments`, and `purposes` names at least one purpose. A caller sees
//!   the object only if they dominate its marking and hold one of its purposes.
//! - A link's `from` and `to` are keys of objects of its link type's declared source and
//!   target types.
//!
//! Parsing is all or nothing: the first bad line refuses the whole file and names the
//! line. [`Import::check_against`] then refuses ids the store already holds, and links
//! whose ends exist neither in the file nor in the store.

use std::collections::{BTreeMap, BTreeSet};

use fabric::{LinkInstance, ObjectInstance, ObjectType, Value, ValueType};
use policy::{Compartment, Marking, Purpose};

use crate::enclosure::level_of;
use crate::json::{parse, Json};
use crate::ontology_file::Ontology;
use crate::strict_json::{identifier, no_repeated_keys, object, only_keys, strings, text};

// The keys each record may carry, public so the published schema
// (`contracts/schema/import-record.v1.schema.json`) can be checked against them in
// `tests/contract_schemas.rs`. [AC-RF-CA-13c]

/// Keys of an object record.
pub const OBJECT_RECORD_KEYS: &[&str] = &[
    "object",
    "key",
    "properties",
    "marking",
    "compartments",
    "purposes",
];
/// Keys of a link record.
pub const LINK_RECORD_KEYS: &[&str] = &["link", "from", "to"];

/// Objects and links read from an import file, checked against the ontology.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    /// Objects, in file order.
    pub objects: Vec<ObjectInstance>,
    /// Links, in file order.
    pub links: Vec<LinkInstance>,
}

impl Import {
    /// Check the import against what the store already holds: refuse an object whose id
    /// exists — an import adds, it never silently replaces — and a link whose ends are
    /// neither in this import nor in the store.
    pub fn check_against(&self, exists_in_store: &dyn Fn(&str) -> bool) -> Result<(), String> {
        if let Some(o) = self.objects.iter().find(|o| exists_in_store(&o.id)) {
            return Err(format!(
                "import refused: {} is already in the enclosure; an import never replaces an \
                 object",
                o.id
            ));
        }
        let imported: BTreeSet<&str> = self.objects.iter().map(|o| o.id.as_str()).collect();
        for l in &self.links {
            for end in [&l.source_id, &l.target_id] {
                if !imported.contains(end.as_str()) && !exists_in_store(end) {
                    return Err(format!(
                        "import refused: link {} from {} to {} names {end}, which is neither \
                         in this import nor in the enclosure",
                        l.link_type, l.source_id, l.target_id
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Parse an import file against `ontology`.
pub fn parse_import(text: &str, ontology: &Ontology) -> Result<Import, String> {
    let mut import = Import {
        objects: Vec::new(),
        links: Vec::new(),
    };
    let mut ids = BTreeSet::new();
    let mut links = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let at = format!("line {}", index + 1);
        record(&at, line, ontology, &mut import, &mut ids, &mut links)
            .map_err(|e| format!("import refused: {e}"))?;
    }
    Ok(import)
}

fn record(
    at: &str,
    line: &str,
    ontology: &Ontology,
    import: &mut Import,
    ids: &mut BTreeSet<String>,
    links: &mut BTreeSet<(String, String, String)>,
) -> Result<(), String> {
    let record = parse(line).map_err(|e| format!("{at}: not valid JSON: {e}"))?;
    match (record.get("object"), record.get("link")) {
        (Some(_), None) => {
            let o = object_record(at, &record, ontology)?;
            if !ids.insert(o.id.clone()) {
                return Err(format!("{at}: {} appears twice in this import", o.id));
            }
            import.objects.push(o);
        }
        (None, Some(_)) => {
            let l = link_record(at, &record, ontology)?;
            let triple = (
                l.link_type.clone(),
                l.source_id.clone(),
                l.target_id.clone(),
            );
            if !links.insert(triple) {
                return Err(format!(
                    "{at}: link {} from {} to {} appears twice in this import",
                    l.link_type, l.source_id, l.target_id
                ));
            }
            import.links.push(l);
        }
        _ => {
            return Err(format!(
                "{at}: a record has exactly one of \"object\" or \"link\""
            ))
        }
    }
    Ok(())
}

fn object_record(at: &str, record: &Json, ontology: &Ontology) -> Result<ObjectInstance, String> {
    only_keys(object(record, at)?, at, OBJECT_RECORD_KEYS)?;
    let type_id = identifier(record, at, "object")?;
    let ty = ontology
        .object_types
        .iter()
        .find(|t| t.id == type_id)
        .ok_or_else(|| {
            format!("{at}: \"{type_id}\" is not an object type the ontology declares")
        })?;
    let id = format!("{type_id}/{}", identifier(record, at, "key")?);
    let at = format!("{at} ({id})");

    let properties = properties(&at, record.get("properties"), ty)?;

    let band = text(record, &at, "marking")?;
    let level = level_of(band).ok_or_else(|| {
        format!(
            "{at}: \"marking\" must be open, internal, restricted or confidential, not \"{band}\""
        )
    })?;
    let compartments: BTreeSet<Compartment> = match record.get("compartments") {
        None => BTreeSet::new(),
        Some(_) => strings(record, &at, "compartments")?
            .into_iter()
            .map(Compartment)
            .collect(),
    };
    let allowed_purposes: BTreeSet<Purpose> = strings(record, &at, "purposes")?
        .into_iter()
        .map(Purpose)
        .collect();
    if allowed_purposes.is_empty() {
        return Err(format!(
            "{at}: \"purposes\" needs at least one purpose, or nobody can ever read it"
        ));
    }

    Ok(ObjectInstance {
        id,
        type_id: ty.id.clone(),
        properties,
        marking: Marking {
            level,
            compartments,
        },
        allowed_purposes,
    })
}

fn properties(
    at: &str,
    item: Option<&Json>,
    ty: &ObjectType,
) -> Result<BTreeMap<String, Value>, String> {
    let fields: &[(String, Json)] = match item {
        None => &[],
        Some(Json::Obj(fields)) => fields,
        Some(other) => {
            return Err(format!(
                "{at}: \"properties\" must be an object, not {}",
                other.encode()
            ))
        }
    };
    no_repeated_keys(fields, &format!("{at}: properties"))?;
    let mut values = BTreeMap::new();
    for (name, value) in fields {
        let declared = ty
            .property(name)
            .ok_or_else(|| format!("{at}: \"{}\" declares no property \"{name}\"", ty.id))?;
        values.insert(name.clone(), typed(at, name, declared.value_type, value)?);
    }
    if let Some(missing) = ty
        .properties
        .iter()
        .find(|p| p.required && !values.contains_key(&p.api_name))
    {
        return Err(format!(
            "{at}: required property \"{}\" is missing",
            missing.api_name
        ));
    }
    Ok(values)
}

/// A JSON value as the property's declared type, without lossy coercion.
fn typed(at: &str, name: &str, value_type: ValueType, value: &Json) -> Result<Value, String> {
    let refused = || {
        format!(
            "{at}: \"{name}\" must be {}, not {}",
            match value_type {
                ValueType::Text => "a string",
                ValueType::Int => "an integer",
                ValueType::Float => "a number",
                ValueType::Bool => "true or false",
                ValueType::Timestamp => "a non-negative integer (Unix seconds)",
            },
            value.encode()
        )
    };
    match (value_type, value) {
        (ValueType::Text, Json::Str(s)) => Ok(Value::Text(s.clone())),
        (ValueType::Int, Json::Int(n)) => Ok(Value::Int(*n)),
        (ValueType::Float, Json::Num(x)) => Ok(Value::Float(*x)),
        (ValueType::Float, Json::Int(n)) => Ok(Value::Float(*n as f64)),
        (ValueType::Bool, Json::Bool(b)) => Ok(Value::Bool(*b)),
        (ValueType::Timestamp, Json::Int(n)) => u64::try_from(*n)
            .map(Value::Timestamp)
            .map_err(|_| refused()),
        _ => Err(refused()),
    }
}

fn link_record(at: &str, record: &Json, ontology: &Ontology) -> Result<LinkInstance, String> {
    only_keys(object(record, at)?, at, LINK_RECORD_KEYS)?;
    let link_id = identifier(record, at, "link")?;
    let link_type = ontology
        .link_types
        .iter()
        .find(|l| l.id == link_id)
        .ok_or_else(|| format!("{at}: \"{link_id}\" is not a link type the ontology declares"))?;
    Ok(LinkInstance {
        link_type: link_type.id.clone(),
        source_id: format!(
            "{}/{}",
            link_type.source_type,
            identifier(record, at, "from")?
        ),
        target_id: format!(
            "{}/{}",
            link_type.target_type,
            identifier(record, at, "to")?
        ),
    })
}
