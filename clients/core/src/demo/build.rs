//! Assemble the canonical dataset body from a fixture directory. `[AC-C-13]`
//! `[NFR-PRV-1]` `[NFR-PQ-3]`
//!
//! Reads `manifest.json`, checks each listed file's SHA-256, and builds
//! `{"format","version","release","inputs","personas","collections"}`. Before the
//! body is returned, every number must have its one spelling ([`number_text`]) and
//! the local-suite guard must pass over the **whole** body; any finding is an `Err`,
//! so nothing that fails either check can reach the signer. The result is the exact
//! byte string the dataset signs.

use std::fs;
use std::path::Path;

use attest::hex::hex;
use attest::json::{parse, Json};
use pqc_agility::sha256;

use super::{canon, guard};

/// Why the body could not be built.
#[derive(Debug, PartialEq)]
pub enum BuildError {
    /// A file is missing or unreadable.
    Io(String),
    /// A file's digest differs from the manifest.
    Digest(String),
    /// A file is not strict JSON, or lacks a required key.
    Shape(String),
    /// A manifest input is not one the dataset may be built from.
    Input(String),
    /// A number is not written the one way [`number_text`] gives.
    Number {
        /// Where.
        path: String,
        /// The text found.
        text: String,
    },
    /// The local-suite guard refused it.
    Guard(Vec<guard::GuardError>),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Io(d) => write!(f, "{d}"),
            BuildError::Digest(d) => write!(f, "digest mismatch: {d}"),
            BuildError::Shape(d) => write!(f, "fixture shape: {d}"),
            BuildError::Input(d) => write!(f, "manifest input: {d}"),
            BuildError::Number { path, text } => write!(
                f,
                "number {text} at {path} is not in its one spelling (write it as JSON.stringify does)"
            ),
            BuildError::Guard(found) => write!(f, "local-suite data refused: {found:?}"),
        }
    }
}

impl std::error::Error for BuildError {}

/// Where a manifest input must come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputOrigin {
    /// `sdk/ts/src/mock/…`: must be byte-identical to the tagged release's copy.
    Tagged,
    /// `clients/demo/…`: must be the committed, clean copy.
    Committed,
}

/// The origin of manifest input `path`, or why it may not be an input. Only plain
/// relative paths under `sdk/ts/src/mock/` or `clients/demo/` are inputs: no empty,
/// `.` or `..` segment, no leading `/`, no backslash.
pub fn input_origin(path: &str) -> Result<InputOrigin, BuildError> {
    let refuse = || {
        BuildError::Input(format!(
            "{path:?} is not a plain path under sdk/ts/src/mock/ or clients/demo/"
        ))
    };
    if path.contains('\\') {
        return Err(refuse());
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        return Err(refuse());
    }
    match parts.as_slice() {
        ["sdk", "ts", "src", "mock", _, ..] => Ok(InputOrigin::Tagged),
        ["clients", "demo", _, ..] => Ok(InputOrigin::Committed),
        _ => Err(refuse()),
    }
}

/// The one spelling of `v`: what ECMAScript's `Number::toString` (and so
/// `JSON.stringify`, which wrote the fixtures) gives — shortest round-trip digits,
/// plain notation for 1e-7 < |v| < 1e21, otherwise `<d>[.<ddd>]e±<n>`, and `0` for
/// both zeros. `None` for a non-finite value, which JSON cannot carry.
pub fn number_text(v: f64) -> Option<String> {
    if !v.is_finite() {
        return None;
    }
    if v == 0.0 {
        return Some("0".into());
    }
    // `{:e}` gives the shortest digits that round-trip, as `d[.ddd]e<exp>`.
    let sci = format!("{:e}", v.abs());
    let (mantissa, exp) = sci.split_once('e')?;
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i64;
    let n = exp.parse::<i64>().ok()? + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        let head = if k == 1 {
            digits.clone()
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        format!("{head}e{sign}{}", e.abs())
    };
    Some(if v < 0.0 { format!("-{body}") } else { body })
}

/// Refuse any number in `v` whose text is not [`number_text`] of its value, so the
/// textual canonical form ([`canon`]) has one body per dataset.
fn check_numbers(v: &Json, path: &str) -> Result<(), BuildError> {
    match v {
        Json::Num(text) => {
            let one = text.parse::<f64>().ok().and_then(number_text);
            if one.as_deref() != Some(text.as_str()) {
                return Err(BuildError::Number {
                    path: path.into(),
                    text: text.clone(),
                });
            }
            Ok(())
        }
        Json::Arr(items) => items
            .iter()
            .enumerate()
            .try_for_each(|(i, x)| check_numbers(x, &format!("{path}[{i}]"))),
        Json::Obj(pairs) => pairs
            .iter()
            .try_for_each(|(k, x)| check_numbers(x, &format!("{path}[{k:?}]"))),
        Json::Null | Json::Bool(_) | Json::Str(_) => Ok(()),
    }
}

fn read_json(dir: &Path, name: &str) -> Result<(Vec<u8>, Json), BuildError> {
    let bytes = fs::read(dir.join(name)).map_err(|e| BuildError::Io(format!("{name}: {e}")))?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| BuildError::Shape(format!("{name}: not UTF-8")))?;
    let v = parse(text).map_err(|e| BuildError::Shape(format!("{name}: {e}")))?;
    Ok((bytes, v))
}

/// A manifest file name: one plain segment, not the manifest itself.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name != "manifest.json"
        && !name.contains(['/', '\\'])
}

/// The canonical dataset body for the fixtures in `dir`.
pub fn canonical_body(dir: &Path) -> Result<Vec<u8>, BuildError> {
    let (_, manifest) = read_json(dir, "manifest.json")?;
    if manifest.get("format").and_then(Json::as_str) != Some("midgard-demo-fixtures") {
        return Err(BuildError::Shape(
            "manifest.json: not a midgard-demo-fixtures manifest".into(),
        ));
    }
    let files = match manifest.get("files") {
        Some(Json::Obj(pairs)) => pairs.clone(),
        _ => return Err(BuildError::Shape("manifest.json: no files".into())),
    };
    let inputs = match manifest.get("inputs") {
        Some(v @ Json::Obj(pairs)) if !pairs.is_empty() => {
            for (path, blob) in pairs {
                input_origin(path)?;
                if blob.as_str().is_none() {
                    return Err(BuildError::Shape(format!(
                        "manifest input {path}: no blob id"
                    )));
                }
            }
            v.clone()
        }
        _ => return Err(BuildError::Shape("manifest.json: no inputs".into())),
    };
    let mut collections: Vec<(String, Json)> = Vec::new();
    let mut personas = None;
    for (name, digest) in &files {
        if !plain_name(name) {
            return Err(BuildError::Shape(format!("manifest names {name:?}")));
        }
        let (bytes, v) = read_json(dir, name)?;
        if digest.as_str() != Some(hex(&sha256(&bytes)).as_str()) {
            return Err(BuildError::Digest(name.clone()));
        }
        if name == "personas.json" {
            personas = Some(v);
            continue;
        }
        match v.get("collections") {
            Some(Json::Obj(pairs)) => {
                for (k, c) in pairs {
                    if collections.iter().any(|(seen, _)| seen == k) {
                        return Err(BuildError::Shape(format!("{name}: {k} appears twice")));
                    }
                    collections.push((k.clone(), c.clone()));
                }
            }
            _ => return Err(BuildError::Shape(format!("{name}: no collections"))),
        }
    }
    let personas =
        personas.ok_or_else(|| BuildError::Shape("manifest.json: no personas.json".into()))?;
    let release = match manifest.get("release") {
        Some(v @ Json::Str(_)) => v.clone(),
        _ => return Err(BuildError::Shape("manifest.json: no release".into())),
    };
    let body = Json::Obj(vec![
        ("format".into(), Json::Str("midgard-demo-dataset".into())),
        ("version".into(), Json::Num("1".into())),
        ("release".into(), release),
        ("inputs".into(), inputs),
        ("personas".into(), personas),
        ("collections".into(), Json::Obj(collections)),
    ]);
    check_numbers(&body, "$")?;
    guard::check(&body).map_err(BuildError::Guard)?;
    Ok(canon::encode(&body).into_bytes())
}
