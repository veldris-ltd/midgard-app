//! Decoding console read models from strict JSON. `[FR-C-1]`
//!
//! Every generated read model implements [`FromJson`]. Objects are decoded strictly:
//! a key the model does not know is an error, so a change to the TypeScript contract
//! that the generator has not been rerun for cannot be silently dropped.

use std::fmt;

use attest::json::Json;

/// Why a value did not decode, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    /// A JSONPath-like location, `$.tracks[2].options[0].rank`.
    pub path: String,
    /// What was wrong.
    pub reason: String,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.reason)
    }
}

impl std::error::Error for DecodeError {}

fn err<T>(path: &str, reason: impl Into<String>) -> Result<T, DecodeError> {
    Err(DecodeError {
        path: path.to_string(),
        reason: reason.into(),
    })
}

/// A value decodable from JSON at `path`.
pub trait FromJson: Sized {
    /// Decode `v`, reporting errors at `path`.
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError>;
}

impl FromJson for String {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Str(s) => Ok(s.clone()),
            _ => err(path, "expected a string"),
        }
    }
}

impl FromJson for bool {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Bool(b) => Ok(*b),
            _ => err(path, "expected a boolean"),
        }
    }
}

impl FromJson for f64 {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Num(text) => text.parse::<f64>().or_else(|_| err(path, "not a number")),
            _ => err(path, "expected a number"),
        }
    }
}

impl FromJson for u32 {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Num(text) => text
                .parse::<u32>()
                .or_else(|_| err(path, "expected a whole number")),
            _ => err(path, "expected a whole number"),
        }
    }
}

impl<T: FromJson> FromJson for Vec<T> {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Arr(items) => items
                .iter()
                .enumerate()
                .map(|(i, x)| T::from_json(x, &format!("{path}[{i}]")))
                .collect(),
            _ => err(path, "expected an array"),
        }
    }
}

/// A `[string, string]` tuple: a two-element array of strings, in order.
impl FromJson for (String, String) {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Arr(items) if items.len() == 2 => Ok((
                String::from_json(&items[0], &format!("{path}[0]"))?,
                String::from_json(&items[1], &format!("{path}[1]"))?,
            )),
            _ => err(path, "expected a two-element array"),
        }
    }
}

/// A `Record<string, string>`: ordered key/value pairs, in document order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyValues(pub Vec<(String, String)>);

impl FromJson for KeyValues {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Obj(pairs) => pairs
                .iter()
                .map(|(k, x)| Ok((k.clone(), String::from_json(x, &format!("{path}.{k}"))?)))
                .collect::<Result<Vec<_>, _>>()
                .map(KeyValues),
            _ => err(path, "expected an object"),
        }
    }
}

/// The pairs of an object whose keys are all in `known`.
pub fn object<'a>(
    v: &'a Json,
    path: &str,
    known: &[&str],
) -> Result<&'a [(String, Json)], DecodeError> {
    match v {
        Json::Obj(pairs) => {
            if let Some((k, _)) = pairs.iter().find(|(k, _)| !known.contains(&k.as_str())) {
                return err(path, format!("unknown key {k:?}"));
            }
            Ok(pairs)
        }
        _ => err(path, "expected an object"),
    }
}

/// A required field.
pub fn field<T: FromJson>(
    pairs: &[(String, Json)],
    key: &str,
    path: &str,
) -> Result<T, DecodeError> {
    match pairs.iter().find(|(k, _)| k == key) {
        Some((_, v)) => T::from_json(v, &format!("{path}.{key}")),
        None => err(path, format!("missing key {key:?}")),
    }
}

/// An optional (`key?:`) or nullable (`T | null`) field: absent or `null` is `None`.
pub fn opt_field<T: FromJson>(
    pairs: &[(String, Json)],
    key: &str,
    path: &str,
) -> Result<Option<T>, DecodeError> {
    match pairs.iter().find(|(k, _)| k == key) {
        None | Some((_, Json::Null)) => Ok(None),
        Some((_, v)) => T::from_json(v, &format!("{path}.{key}")).map(Some),
    }
}
