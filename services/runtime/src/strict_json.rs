//! Strict readers over parsed JSON, shared by the deployment files Midgard loads — the
//! ontology and data imports. Each refuses rather than guesses: a misspelt key would
//! otherwise vanish, and a repeated one would be read only once. Errors name where in
//! the file they are (`at`). [NFR-SEC-2]

use std::collections::BTreeSet;

use crate::json::Json;

/// The fields of an object, or an error if `item` is not one.
pub fn object<'a>(item: &'a Json, at: &str) -> Result<&'a [(String, Json)], String> {
    match item {
        Json::Obj(fields) => Ok(fields),
        _ => Err(format!("{at} must be an object")),
    }
}

/// Refuse unknown and repeated keys.
pub fn only_keys(fields: &[(String, Json)], at: &str, allowed: &[&str]) -> Result<(), String> {
    no_repeated_keys(fields, at)?;
    match fields
        .iter()
        .find(|(key, _)| !allowed.contains(&key.as_str()))
    {
        Some((key, _)) => Err(format!("{at}: unknown key \"{key}\"")),
        None => Ok(()),
    }
}

/// Refuse repeated keys, whatever they are.
pub fn no_repeated_keys(fields: &[(String, Json)], at: &str) -> Result<(), String> {
    for (i, (key, _)) in fields.iter().enumerate() {
        if fields[..i].iter().any(|(earlier, _)| earlier == key) {
            return Err(format!("{at}: \"{key}\" appears twice"));
        }
    }
    Ok(())
}

/// A required, non-blank string.
pub fn text<'a>(item: &'a Json, at: &str, key: &str) -> Result<&'a str, String> {
    match item.get(key) {
        Some(Json::Str(s)) if !s.trim().is_empty() => Ok(s.as_str()),
        Some(Json::Str(_)) => Err(format!("{at}: \"{key}\" is empty")),
        Some(other) => Err(format!(
            "{at}: \"{key}\" must be a string, not {}",
            other.encode()
        )),
        None => Err(format!("{at}: missing \"{key}\"")),
    }
}

/// A required id, api name or key: letters, digits, `_` and `-` only, so it can be
/// joined into a `<type>/<key>` object id without ambiguity.
pub fn identifier<'a>(item: &'a Json, at: &str, key: &str) -> Result<&'a str, String> {
    let value = text(item, at, key)?;
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Ok(value)
    } else {
        Err(format!(
            "{at}: \"{key}\" may contain only letters, digits, _ and -, not \"{value}\""
        ))
    }
}

/// An optional boolean, `default` when absent.
pub fn optional_bool(item: &Json, at: &str, key: &str, default: bool) -> Result<bool, String> {
    match item.get(key) {
        None => Ok(default),
        Some(Json::Bool(b)) => Ok(*b),
        Some(other) => Err(format!(
            "{at}: \"{key}\" must be true or false, not {}",
            other.encode()
        )),
    }
}

/// A required array.
pub fn array<'a>(item: &'a Json, at: &str, key: &str) -> Result<&'a [Json], String> {
    match item.get(key) {
        Some(Json::Arr(items)) => Ok(items),
        Some(other) => Err(format!(
            "{at}: \"{key}\" must be an array, not {}",
            other.encode()
        )),
        None => Err(format!("{at}: missing \"{key}\"")),
    }
}

/// An optional array, empty when absent.
pub fn optional_array<'a>(item: &'a Json, at: &str, key: &str) -> Result<&'a [Json], String> {
    match item.get(key) {
        None => Ok(&[]),
        Some(_) => array(item, at, key),
    }
}

/// A required array of non-blank strings.
pub fn strings(item: &Json, at: &str, key: &str) -> Result<Vec<String>, String> {
    array(item, at, key)?
        .iter()
        .map(|v| match v {
            Json::Str(s) if !s.trim().is_empty() => Ok(s.clone()),
            other => Err(format!(
                "{at}: \"{key}\" must hold non-empty strings, not {}",
                other.encode()
            )),
        })
        .collect()
}

/// Refuse a repeated value.
pub fn unique<'a>(values: impl Iterator<Item = &'a str>, what: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(format!("duplicate {what} \"{value}\""));
        }
    }
    Ok(())
}
