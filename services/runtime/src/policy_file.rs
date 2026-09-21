//! The policy file: a deployment's discretionary rule set, loaded at startup rather
//! than compiled in. [NFR-SEC-2] [AC-M-4]
//!
//! ```json
//! {
//!   "version": 1,
//!   "rules": [
//!     { "id": "operator-read", "role": "operator", "action": "Read",
//!       "resource": "*", "effect": "permit" },
//!     { "id": "no-export", "role": "analyst", "action": "Read",
//!       "purpose": "export", "resource": "object-type:aircraft", "effect": "deny" }
//!   ]
//! }
//! ```
//!
//! Rules are discretionary only. The PDP evaluates mandatory access control
//! (clearance and compartments) and the purpose gate before any rule, an explicit
//! deny overrides a permit, and anything no rule permits is denied — so an empty rule
//! set denies everything.
//!
//! Loading is strict. A misspelt key would otherwise disappear silently and change
//! what is enforced, so unknown or repeated keys, missing or empty fields, wrong
//! types, an unknown effect or version, and duplicate rule ids all refuse the whole
//! file. `"resource": "*"` means any resource; `"purpose"` is optional and, when
//! absent, the rule applies under any purpose.

use std::fmt;

use policy::{
    Action, Effect, PolicyError, PolicySet, Purpose, ResourceId, Role, Rule, RuleId, Selector,
};

use crate::json::{parse, Json};

/// The one policy file format version this build reads and writes.
pub const POLICY_FILE_VERSION: i64 = 1;

/// The `resource` value that selects every resource.
const ANY_RESOURCE: &str = "*";

/// Why a policy file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyFileError {
    /// The file is not valid JSON.
    Syntax(String),
    /// Valid JSON, but not a policy file this build can read exactly.
    Shape(String),
    /// Well-formed rules that do not make a valid policy set.
    Policy(PolicyError),
}

impl fmt::Display for PolicyFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyFileError::Syntax(e) => write!(f, "policy file is not valid JSON: {e}"),
            PolicyFileError::Shape(e) => write!(f, "policy file refused: {e}"),
            PolicyFileError::Policy(e) => write!(f, "policy file refused: {e}"),
        }
    }
}

impl std::error::Error for PolicyFileError {}

fn shape(message: String) -> PolicyFileError {
    PolicyFileError::Shape(message)
}

/// Parse a policy file into the rule set the PDP enforces.
pub fn parse_policy(text: &str) -> Result<PolicySet, PolicyFileError> {
    let doc = parse(text).map_err(PolicyFileError::Syntax)?;
    let Json::Obj(fields) = &doc else {
        return Err(shape("the policy file must be a JSON object".into()));
    };
    only_keys(fields, "the policy file", &["version", "rules"])?;

    match doc.get("version") {
        Some(Json::Int(v)) if *v == POLICY_FILE_VERSION => {}
        Some(Json::Num(v)) if *v == POLICY_FILE_VERSION as f64 => {}
        Some(other) => {
            return Err(shape(format!(
                "unsupported version {} (this build reads version {POLICY_FILE_VERSION})",
                other.encode()
            )))
        }
        None => return Err(shape("missing \"version\"".into())),
    }

    let Some(Json::Arr(items)) = doc.get("rules") else {
        return Err(shape("\"rules\" must be an array".into()));
    };
    let rules = items
        .iter()
        .enumerate()
        .map(|(index, item)| rule(index, item))
        .collect::<Result<Vec<_>, _>>()?;
    PolicySet::new(rules).map_err(PolicyFileError::Policy)
}

/// Render a rule set as a policy file that [`parse_policy`] reads back to the same
/// rules.
///
/// Refuses a rule selecting a resource whose id is literally `*`: written out, it
/// would load back as "any resource" and silently broaden what the rule covers.
pub fn render_policy(policies: &PolicySet) -> Result<String, PolicyFileError> {
    let mut rules = Vec::with_capacity(policies.rules().len());
    for r in policies.rules() {
        let resource = match &r.selector {
            Selector::Any => ANY_RESOURCE,
            Selector::Resource(id) if id.0 == ANY_RESOURCE => {
                return Err(shape(format!(
                    "rule \"{}\" selects a resource named \"{ANY_RESOURCE}\", which a policy \
                     file cannot tell apart from any resource",
                    r.id.0
                )))
            }
            Selector::Resource(id) => id.0.as_str(),
        };
        let mut pairs = vec![
            ("id", Json::s(&r.id.0)),
            ("role", Json::s(&r.role.0)),
            ("action", Json::s(&r.action.0)),
        ];
        if let Some(purpose) = &r.purpose {
            pairs.push(("purpose", Json::s(&purpose.0)));
        }
        pairs.push(("resource", Json::s(resource)));
        pairs.push((
            "effect",
            Json::s(match r.effect {
                Effect::Permit => "permit",
                Effect::Deny => "deny",
            }),
        ));
        rules.push(Json::obj(pairs));
    }
    Ok(Json::obj(vec![
        ("version", Json::Int(POLICY_FILE_VERSION)),
        ("rules", Json::Arr(rules)),
    ])
    .encode())
}

fn rule(index: usize, item: &Json) -> Result<Rule, PolicyFileError> {
    let at = format!("rules[{index}]");
    let Json::Obj(fields) = item else {
        return Err(shape(format!("{at} must be an object")));
    };
    only_keys(
        fields,
        &at,
        &["id", "role", "action", "purpose", "resource", "effect"],
    )?;

    let id = text(item, &at, "id")?;
    let at = format!("rule \"{id}\"");
    let effect = match text(item, &at, "effect")? {
        "permit" => Effect::Permit,
        "deny" => Effect::Deny,
        other => {
            return Err(shape(format!(
                "{at}: \"effect\" must be \"permit\" or \"deny\", not \"{other}\""
            )))
        }
    };
    let purpose = match item.get("purpose") {
        None => None,
        Some(_) => Some(Purpose(text(item, &at, "purpose")?.to_string())),
    };
    let selector = match text(item, &at, "resource")? {
        ANY_RESOURCE => Selector::Any,
        resource => Selector::Resource(ResourceId(resource.to_string())),
    };
    Ok(Rule {
        id: RuleId(id.to_string()),
        role: Role(text(item, &at, "role")?.to_string()),
        action: Action(text(item, &at, "action")?.to_string()),
        purpose,
        selector,
        effect,
    })
}

/// A required, non-blank string field.
fn text<'a>(item: &'a Json, at: &str, key: &str) -> Result<&'a str, PolicyFileError> {
    match item.get(key) {
        Some(Json::Str(s)) if !s.trim().is_empty() => Ok(s.as_str()),
        Some(Json::Str(_)) => Err(shape(format!("{at}: \"{key}\" is empty"))),
        Some(other) => Err(shape(format!(
            "{at}: \"{key}\" must be a string, not {}",
            other.encode()
        ))),
        None => Err(shape(format!("{at}: missing \"{key}\""))),
    }
}

/// Refuse unknown and repeated keys: `Json::get` would otherwise read only the first
/// of two same-named keys, and ignore a misspelt one entirely.
fn only_keys(fields: &[(String, Json)], at: &str, allowed: &[&str]) -> Result<(), PolicyFileError> {
    for (i, (key, _)) in fields.iter().enumerate() {
        if !allowed.contains(&key.as_str()) {
            return Err(shape(format!("{at}: unknown key \"{key}\"")));
        }
        if fields[..i].iter().any(|(earlier, _)| earlier == key) {
            return Err(shape(format!("{at}: \"{key}\" appears twice")));
        }
    }
    Ok(())
}
