//! The classification rules a deployment gives `heimdall-feed`, read strictly from
//! `rules.json` in its state directory. [FR-H-2] [AC-H-6]
//!
//! ```json
//! { "version": 1,
//!   "rules": [
//!     { "label": "emergency", "rationale": "squawk 7700",
//!       "when": { "attr": "squawk", "equals": "7700" } },
//!     { "label": "fast-mover", "rationale": "ground speed over 400 kt",
//!       "when": { "all": [ { "kind": "air-track" }, { "attr": "speed", "gt": 400 } ] } }
//!   ] }
//! ```
//!
//! A condition is exactly one of `{"kind": K}`, `{"has": A}`, `{"attr": A, "gt": N}`,
//! `{"attr": A, "lt": N}`, `{"attr": A, "equals": V}` (a string, whole number, number or
//! boolean, compared with the attribute's own type), `{"all": [..]}`, `{"any": [..]}`
//! or `{"not": C}`. Rules apply in order and the first that matches labels the entity.
//! Unknown keys, an empty `all`/`any`, a missing label or rationale and a repeated label
//! all refuse the file, so a typo cannot silently classify nothing.

use std::collections::BTreeSet;

use heimdall_classify::{Condition, Rule, RulesClassifier};
use heimdall_picture::Attr;
use service_runtime::json::{parse, Json};

/// The one rules file version this build reads.
const VERSION: i64 = 1;

/// Parse a rules file into an ordered classifier.
pub fn parse_rules(text: &str) -> Result<RulesClassifier, String> {
    let doc = parse(text)?;
    only_keys(&doc, "the rules file", &["version", "rules"])?;
    match doc.get("version").and_then(Json::as_i64) {
        Some(VERSION) => {}
        other => {
            return Err(format!(
                "rules file version must be {VERSION}, not {other:?}"
            ))
        }
    }
    let Some(Json::Arr(items)) = doc.get("rules") else {
        return Err("the rules file needs a \"rules\" array".into());
    };
    let mut classifier = RulesClassifier::new();
    let mut labels = BTreeSet::new();
    for (i, item) in items.iter().enumerate() {
        let at = format!("rule {}", i + 1);
        only_keys(item, &at, &["label", "rationale", "when"])?;
        let label = text_field(item, &at, "label")?;
        let rationale = text_field(item, &at, "rationale")?;
        if !labels.insert(label.to_string()) {
            return Err(format!("{at}: the label '{label}' is used twice"));
        }
        let when = item
            .get("when")
            .ok_or_else(|| format!("{at}: needs \"when\""))?;
        classifier = classifier.rule(Rule::new(label, rationale, condition(when, &at)?));
    }
    Ok(classifier)
}

fn condition(c: &Json, at: &str) -> Result<Condition, String> {
    let Json::Obj(pairs) = c else {
        return Err(format!("{at}: a condition must be an object"));
    };
    let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
    let attr = || text_field(c, at, "attr").map(str::to_string);
    let number = |key: &str| match c.get(key) {
        Some(Json::Int(i)) => Ok(*i as f64),
        Some(Json::Num(n)) => Ok(*n),
        _ => Err(format!("{at}: \"{key}\" must be a number")),
    };
    match keys.as_slice() {
        ["kind"] => Ok(Condition::KindIs(text_field(c, at, "kind")?.to_string())),
        ["has"] => Ok(Condition::HasAttr(text_field(c, at, "has")?.to_string())),
        ["attr", "gt"] | ["gt", "attr"] => Ok(Condition::AttrGreaterThan(attr()?, number("gt")?)),
        ["attr", "lt"] | ["lt", "attr"] => Ok(Condition::AttrLessThan(attr()?, number("lt")?)),
        ["attr", "equals"] | ["equals", "attr"] => {
            let value = match c.get("equals") {
                Some(Json::Str(s)) => Attr::Text(s.clone()),
                Some(Json::Int(i)) => Attr::Int(*i),
                Some(Json::Num(n)) => Attr::Float(*n),
                Some(Json::Bool(b)) => Attr::Bool(*b),
                _ => {
                    return Err(format!(
                        "{at}: \"equals\" must be a string, number or boolean"
                    ))
                }
            };
            Ok(Condition::AttrEquals(attr()?, value))
        }
        ["all"] | ["any"] => {
            let Some(Json::Arr(parts)) = c.get(keys[0]) else {
                return Err(format!("{at}: \"{}\" must be an array", keys[0]));
            };
            let mut parts = parts.iter().map(|p| condition(p, at));
            let first = parts
                .next()
                .ok_or_else(|| format!("{at}: \"{}\" must not be empty", keys[0]))??;
            parts.try_fold(first, |acc, next| {
                let next = next?;
                Ok(if keys[0] == "all" {
                    Condition::And(Box::new(acc), Box::new(next))
                } else {
                    Condition::Or(Box::new(acc), Box::new(next))
                })
            })
        }
        ["not"] => Ok(Condition::Not(Box::new(condition(
            c.get("not").unwrap_or(&Json::Null),
            at,
        )?))),
        _ => Err(format!(
            "{at}: a condition is one of kind, has, attr+gt, attr+lt, attr+equals, all, any \
             or not; found {keys:?}"
        )),
    }
}

fn text_field<'a>(item: &'a Json, at: &str, key: &str) -> Result<&'a str, String> {
    match item.str_field(key) {
        Some(s) if !s.trim().is_empty() => Ok(s),
        _ => Err(format!("{at}: \"{key}\" must be a non-empty string")),
    }
}

fn only_keys(item: &Json, at: &str, allowed: &[&str]) -> Result<(), String> {
    let Json::Obj(pairs) = item else {
        return Err(format!("{at} must be an object"));
    };
    for (key, _) in pairs {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{at}: unknown key \"{key}\""));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use heimdall_classify::Classifier;
    use heimdall_picture::{EntityId, LayerId, Track, TrackedEntity};

    use super::*;

    fn entity(kind: &str, attrs: &[(&str, Attr)]) -> TrackedEntity {
        TrackedEntity {
            id: EntityId("E".into()),
            entity_key: "E".into(),
            kind: kind.into(),
            layer: LayerId("air".into()),
            sources: BTreeSet::new(),
            track: Track::opened_at(1),
            classification: None,
            latest_attrs: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    const RULES: &str = r#"{ "version": 1, "rules": [
        { "label": "emergency", "rationale": "squawk 7700",
          "when": { "attr": "squawk", "equals": "7700" } },
        { "label": "rotorcraft", "rationale": "ADS-B category A7",
          "when": { "all": [ { "kind": "air-track" }, { "attr": "category", "equals": "A7" } ] } },
        { "label": "fast-mover", "rationale": "over 400 kt",
          "when": { "any": [ { "attr": "speed", "gt": 400 }, { "has": "afterburner" } ] } },
        { "label": "surface", "rationale": "on the ground",
          "when": { "not": { "attr": "on_ground", "equals": false } } }
    ] }"#;

    #[test]
    fn rules_classify_in_order_with_their_rationale() {
        let rules = parse_rules(RULES).unwrap();
        let label = |e: &TrackedEntity| rules.classify(e).map(|c| (c.label, c.rationale));

        let emergency = entity(
            "air-track",
            &[
                ("squawk", Attr::Text("7700".into())),
                ("speed", Attr::Float(450.0)),
            ],
        );
        assert_eq!(
            label(&emergency),
            Some(("emergency".into(), "squawk 7700".into()))
        );
        let heli = entity("air-track", &[("category", Attr::Text("A7".into()))]);
        assert_eq!(label(&heli).unwrap().0, "rotorcraft");
        let jet = entity(
            "air-track",
            &[("speed", Attr::Int(446)), ("on_ground", Attr::Bool(false))],
        );
        assert_eq!(label(&jet).unwrap().0, "fast-mover");
        let parked = entity("air-track", &[("on_ground", Attr::Bool(true))]);
        assert_eq!(label(&parked).unwrap().0, "surface");
        let cruise = entity(
            "air-track",
            &[
                ("speed", Attr::Float(250.0)),
                ("on_ground", Attr::Bool(false)),
            ],
        );
        assert_eq!(label(&cruise), None);
        assert_eq!(
            label(&entity("air-track", &[("squawk", Attr::Int(7700))])).map(|l| l.0),
            Some("surface".into()),
            "equals compares types: the integer 7700 is not the text \"7700\""
        );
    }

    #[test]
    fn a_malformed_file_is_refused() {
        for (bad, why) in [
            (r#"{"version": 2, "rules": []}"#, "version"),
            (r#"{"version": 1, "rules": [], "extra": 1}"#, "unknown key"),
            (
                r#"{"version": 1, "rules": [{"label": "x", "when": {"has": "a"}}]}"#,
                "rationale",
            ),
            (
                r#"{"version": 1, "rules": [{"label": "x", "rationale": "y", "when": {"attr": "a", "gte": 1}}]}"#,
                "condition",
            ),
            (
                r#"{"version": 1, "rules": [{"label": "x", "rationale": "y", "when": {"all": []}}]}"#,
                "empty",
            ),
            (
                r#"{"version": 1, "rules": [{"label": "x", "rationale": "y", "when": {"has": "a"}},
                                          {"label": "x", "rationale": "z", "when": {"has": "b"}}]}"#,
                "twice",
            ),
        ] {
            let err = parse_rules(bad)
                .err()
                .unwrap_or_else(|| panic!("accepted: {bad}"));
            assert!(err.contains(why), "{why}: {err}");
        }
        assert!(parse_rules(r#"{"version": 1, "rules": []}"#).is_ok());
    }
}
