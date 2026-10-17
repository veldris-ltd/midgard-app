//! Track time-to-live by kind, read strictly from `ttl.json` in the state directory.
//! [FR-H-1] [FR-H-2]
//!
//! ```json
//! { "version": 1,
//!   "ttl": { "air-track": 120, "sea-vessel": 1800, "event-incident": 0 } }
//! ```
//!
//! Each key is an exact entity kind and each value whole seconds; `0` keeps that kind
//! for ever. Kinds not listed use `--track-ttl`. Unknown keys, a negative or fractional
//! number, an empty kind and a kind given twice all refuse the file.

use std::collections::BTreeMap;

use service_runtime::json::{parse, Json};

/// The one TTL file version this build reads.
const VERSION: i64 = 1;

/// Parse a TTL file into seconds by exact kind.
pub fn parse_ttl(text: &str) -> Result<BTreeMap<String, u64>, String> {
    let doc = parse(text)?;
    let Json::Obj(pairs) = &doc else {
        return Err("the ttl file must be an object".into());
    };
    for (key, _) in pairs {
        if key != "version" && key != "ttl" {
            return Err(format!("the ttl file: unknown key \"{key}\""));
        }
    }
    match doc.get("version").and_then(Json::as_i64) {
        Some(VERSION) => {}
        other => return Err(format!("ttl file version must be {VERSION}, not {other:?}")),
    }
    let Some(Json::Obj(kinds)) = doc.get("ttl") else {
        return Err("the ttl file needs a \"ttl\" object".into());
    };
    let mut out = BTreeMap::new();
    for (kind, value) in kinds {
        if kind.trim().is_empty() {
            return Err("the ttl file: a kind must be a non-empty string".into());
        }
        let seconds = match value {
            Json::Int(n) if *n >= 0 => *n as u64,
            _ => {
                return Err(format!(
                    "the ttl for \"{kind}\" must be a whole number of seconds (0: never)"
                ))
            }
        };
        if out.insert(kind.clone(), seconds).is_some() {
            return Err(format!("the ttl file: \"{kind}\" is given twice"));
        }
    }
    Ok(out)
}

/// The TTLs as one log field: `air-track=120,sea-vessel=1800` (empty for none).
pub fn describe_ttl(ttl: &BTreeMap<String, u64>) -> String {
    ttl.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ttl_file_parses_by_exact_kind() {
        let ttl =
            parse_ttl(r#"{"version": 1, "ttl": {"air-track": 120, "sea-vessel": 0}}"#).unwrap();
        assert_eq!(ttl.get("air-track"), Some(&120));
        assert_eq!(ttl.get("sea-vessel"), Some(&0));
        assert_eq!(describe_ttl(&ttl), "air-track=120,sea-vessel=0");
        assert!(parse_ttl(r#"{"version": 1, "ttl": {}}"#)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_malformed_ttl_file_is_refused() {
        for (bad, why) in [
            (r#"{"version": 2, "ttl": {}}"#, "version"),
            (r#"{"ttl": {}}"#, "version"),
            (r#"{"version": 1}"#, "needs"),
            (r#"{"version": 1, "ttl": [], "x": 1}"#, "unknown key"),
            (
                r#"{"version": 1, "ttl": {"air-track": -1}}"#,
                "whole number",
            ),
            (
                r#"{"version": 1, "ttl": {"air-track": 1.5}}"#,
                "whole number",
            ),
            (
                r#"{"version": 1, "ttl": {"air-track": "60"}}"#,
                "whole number",
            ),
            (r#"{"version": 1, "ttl": {" ": 60}}"#, "non-empty"),
            (r#"{"version": 1, "ttl": {"a": 1, "a": 2}}"#, "twice"),
            ("[]", "object"),
        ] {
            let err = parse_ttl(bad)
                .err()
                .unwrap_or_else(|| panic!("accepted: {bad}"));
            assert!(err.contains(why), "{why}: {err}");
        }
    }
}
