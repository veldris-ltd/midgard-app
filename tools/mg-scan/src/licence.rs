//! The licence allow-list, and evaluating SPDX expressions against it.
//!
//! ## Why expressions, not string equality
//! Dual licensing is the norm in the Rust ecosystem: 34 of this workspace's 97
//! components are `MIT OR Apache-2.0` or a permutation. Comparing the whole
//! expression as an opaque token would reject nearly everything and push
//! maintainers straight to a blanket exception, which defeats the gate.
//!
//! So the expression is evaluated: `OR` needs one side satisfiable, `AND` needs
//! both, `WITH` is an exception modifier that relaxes the licence it attaches to.
//! `MIT/Apache-2.0` — a legacy slash form still in the wild — is read as `OR`.
//!
//! Licences and `WITH` modifiers are separate lists (`[allowed]` and
//! `[modifiers]`). A modifier such as `LLVM-exception` is not a licence: it is
//! accepted only to the right of `WITH`, never as the whole of a component's
//! terms.
//!
//! Anything that fails to parse is a refusal, never a pass.

use std::collections::BTreeMap;

/// A parsed `supply-chain/licences.toml`.
#[derive(Debug, Default)]
pub struct Policy {
    allowed: Vec<String>,
    /// Exception modifiers accepted to the right of `WITH`.
    modifiers: Vec<String>,
    /// crate name -> (permitted expression, owner, expiry)
    exceptions: BTreeMap<String, Exception>,
}

/// One recorded licence exception.
#[derive(Debug, Clone)]
pub struct Exception {
    pub licence: String,
    pub owner: String,
    pub expires: String,
}

impl Policy {
    /// Parse the policy file.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut policy = Policy::default();
        let mut section = String::new();
        let mut current_exception: Option<String> = None;
        let mut pending: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();

        for (n, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim().to_string();
            if line.is_empty() {
                continue;
            }
            if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = header.to_string();
                current_exception = header
                    .strip_prefix("exceptions.")
                    .map(|c| c.trim_matches('"').to_string());
                if let Some(c) = &current_exception {
                    pending.entry(c.clone()).or_default();
                }
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("line {}: expected `key = value`", n + 1));
            };
            let key = key.trim().trim_matches('"').to_string();
            let value = value.trim().trim_matches('"').to_string();

            if section == "allowed" || section == "modifiers" {
                // Each entry carries a one-line justification. An identifier
                // nobody could say why they allowed is not a decision.
                if value.trim().is_empty() {
                    return Err(format!("line {}: {key} has no justification", n + 1));
                }
                if section == "allowed" {
                    policy.allowed.push(key);
                } else {
                    policy.modifiers.push(key);
                }
            } else if let Some(c) = &current_exception {
                pending.entry(c.clone()).or_default().insert(key, value);
            } else {
                return Err(format!(
                    "line {}: `{key}` is not inside [allowed], [modifiers] or an \
                     [exceptions.\"<crate>\"] table",
                    n + 1
                ));
            }
        }

        for (crate_name, fields) in pending {
            // Present AND saying something: an empty owner or reason is not a
            // decision anyone made.
            let need = |k: &str| -> Result<String, String> {
                match fields.get(k).map(|v| v.trim()) {
                    None => Err(format!("licence exception for {crate_name} has no `{k}`")),
                    Some("") => Err(format!(
                        "licence exception for {crate_name} has an empty `{k}`"
                    )),
                    Some(v) => Ok(v.to_string()),
                }
            };
            need("reason")?;
            policy.exceptions.insert(
                crate_name.clone(),
                Exception {
                    licence: need("licence").or_else(|_| need("license"))?,
                    owner: need("owner")?,
                    expires: need("expires")?,
                },
            );
        }
        Ok(policy)
    }

    /// Whether a single SPDX identifier is allowed outright.
    fn allows_id(&self, id: &str) -> bool {
        self.allowed.iter().any(|a| a.eq_ignore_ascii_case(id))
    }

    /// Whether an identifier is an accepted `WITH` modifier.
    fn allows_modifier(&self, id: &str) -> bool {
        self.modifiers.iter().any(|a| a.eq_ignore_ascii_case(id))
    }

    /// Whether an SPDX expression is satisfiable under this policy.
    pub fn allows(&self, expression: &str) -> bool {
        evaluate(expression, &|id| self.allows_id(id), &|id| {
            self.allows_modifier(id)
        })
    }

    /// A recorded exception for a crate, if any.
    pub fn exception(&self, crate_name: &str) -> Option<&Exception> {
        self.exceptions.get(crate_name)
    }

    /// Every exception, for expiry checking.
    pub fn exceptions(&self) -> impl Iterator<Item = (&String, &Exception)> {
        self.exceptions.iter()
    }

    /// How many identifiers are allowed. An empty policy is a configuration
    /// error, not a permissive one.
    pub fn allowed_count(&self) -> usize {
        self.allowed.len()
    }
}

fn strip_comment(line: &str) -> &str {
    // Comments only start a line here; a `#` inside a quoted justification must
    // survive. Splitting naively on '#' would truncate reasons.
    if line.trim_start().starts_with('#') {
        ""
    } else {
        line
    }
}

/// Evaluate an SPDX expression, with `allowed` deciding licence identifiers and
/// `modifier` deciding what may follow `WITH`.
///
/// Precedence: `OR` binds loosest, then `AND`, then `WITH`. Parentheses group.
pub fn evaluate(
    expr: &str,
    allowed: &dyn Fn(&str) -> bool,
    modifier: &dyn Fn(&str) -> bool,
) -> bool {
    let e = expr.trim();
    if e.is_empty() {
        return false;
    }
    if let Some(inner) = strip_outer_parens(e) {
        return evaluate(inner, allowed, modifier);
    }
    // The legacy `MIT/Apache-2.0` form means OR.
    if let Some(parts) = split_top_level(e, &["/"]) {
        if parts.len() > 1 {
            return parts.iter().any(|p| evaluate(p, allowed, modifier));
        }
    }
    if let Some(parts) = split_top_level(e, &[" OR "]) {
        if parts.len() > 1 {
            return parts.iter().any(|p| evaluate(p, allowed, modifier));
        }
    }
    if let Some(parts) = split_top_level(e, &[" AND "]) {
        if parts.len() > 1 {
            return parts.iter().all(|p| evaluate(p, allowed, modifier));
        }
    }
    if let Some(parts) = split_top_level(e, &[" WITH "]) {
        if parts.len() == 2 {
            // An exception only ever relaxes the licence it modifies, so the
            // pair is acceptable if the base licence is — provided the exception
            // itself is one we recognise.
            return evaluate(parts[0], allowed, modifier) && modifier(parts[1].trim());
        }
    }
    allowed(e)
}

fn strip_outer_parens(e: &str) -> Option<&str> {
    let inner = e.strip_prefix('(')?.strip_suffix(')')?;
    // Only strip when the parens actually wrap the whole expression.
    let mut depth = 0i32;
    for c in inner.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(inner)
}

/// Split on a separator, ignoring anything inside parentheses.
fn split_top_level<'a>(e: &'a str, seps: &[&str]) -> Option<Vec<&'a str>> {
    for sep in seps {
        let mut parts = Vec::new();
        let mut depth = 0i32;
        let mut start = 0usize;
        let bytes = e.as_bytes();
        let mut i = 0usize;
        while i < e.len() {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth == 0 && e[i..].starts_with(sep) {
                parts.push(&e[start..i]);
                i += sep.len();
                start = i;
                continue;
            }
            i += 1;
        }
        parts.push(&e[start..]);
        if parts.len() > 1 {
            return Some(parts);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = r#"
[allowed]
"Apache-2.0" = "ours"
"MIT" = "permissive"
"BSD-3-Clause" = "permissive"
"Unicode-3.0" = "data"

[modifiers]
"LLVM-exception" = "relaxes Apache-2.0"

[exceptions]
"#;

    fn policy() -> Policy {
        Policy::parse(POLICY).expect("parses")
    }

    #[test]
    fn accepts_the_expressions_this_workspace_actually_uses() {
        let p = policy();
        for e in [
            "Apache-2.0",
            "MIT",
            "MIT OR Apache-2.0",
            "Apache-2.0 OR MIT",
            "MIT/Apache-2.0",
            "BSD-3-Clause",
            "MIT OR Apache-2.0 OR BSD-1-Clause",
            "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
            "(MIT OR Apache-2.0) AND Unicode-3.0",
        ] {
            assert!(p.allows(e), "should allow {e:?}");
        }
    }

    #[test]
    fn refuses_copyleft_and_unknowns() {
        let p = policy();
        for e in [
            "GPL-3.0",
            "AGPL-3.0",
            "LGPL-2.1",
            "MPL-2.0",
            "NOASSERTION",
            "",
        ] {
            assert!(!p.allows(e), "should refuse {e:?}");
        }
    }

    #[test]
    fn or_needs_one_side_and_and_needs_both() {
        let p = policy();
        assert!(
            p.allows("GPL-3.0 OR MIT"),
            "OR: one acceptable side is enough"
        );
        assert!(
            !p.allows("GPL-3.0 AND MIT"),
            "AND: every term must be allowed"
        );
        assert!(
            !p.allows("(GPL-3.0 OR AGPL-3.0) AND MIT"),
            "an unsatisfiable OR poisons the AND"
        );
    }

    #[test]
    fn an_unrecognised_with_exception_is_refused() {
        let p = policy();
        assert!(p.allows("Apache-2.0 WITH LLVM-exception"));
        assert!(
            !p.allows("Apache-2.0 WITH Some-Unknown-Exception"),
            "an unknown exception modifier could relax terms arbitrarily"
        );
    }

    #[test]
    fn exceptions_require_owner_reason_and_expiry() {
        let with_exception = format!(
            "{POLICY}\n[exceptions.\"crate-x\"]\nlicence = \"MPL-2.0\"\nowner = \"a.name\"\nreason = \"file-level copyleft, unmodified\"\nexpires = \"2026-12-31\"\n"
        );
        let p = Policy::parse(&with_exception).expect("parses");
        let e = p.exception("crate-x").expect("recorded");
        assert_eq!(e.licence, "MPL-2.0");
        assert_eq!(e.owner, "a.name");
        assert_eq!(e.expires, "2026-12-31");

        // Each field must be present and must say something.
        for (tag, from, to) in [
            ("no owner", "owner = \"a.name\"\n", ""),
            ("an empty owner", "\"a.name\"", "\"\""),
            (
                "no reason",
                "reason = \"file-level copyleft, unmodified\"\n",
                "",
            ),
            (
                "a blank reason",
                "\"file-level copyleft, unmodified\"",
                "\"  \"",
            ),
            ("no expiry", "expires = \"2026-12-31\"\n", ""),
        ] {
            assert!(
                Policy::parse(&with_exception.replace(from, to)).is_err(),
                "an exception with {tag} is not a decision anyone made"
            );
        }
    }

    #[test]
    fn a_with_modifier_is_not_a_licence() {
        let p = policy();
        assert!(p.allows("Apache-2.0 WITH LLVM-exception"));
        assert!(
            !p.allows("LLVM-exception"),
            "a modifier alone is not terms anyone can ship under"
        );
        assert!(
            !p.allows("LLVM-exception OR GPL-3.0"),
            "nor is it a satisfiable alternative"
        );
        assert!(
            !p.allows("Apache-2.0 WITH MIT"),
            "and a licence is not a modifier"
        );
    }

    #[test]
    fn every_allowed_identifier_must_be_justified() {
        assert!(Policy::parse("[allowed]\n\"MIT\" = \"\"\n").is_err());
        assert!(Policy::parse("[modifiers]\n\"LLVM-exception\" = \"  \"\n").is_err());
        assert!(
            Policy::parse("\"MIT\" = \"stray\"\n").is_err(),
            "an entry outside any table is a mistake, not something to ignore"
        );
    }

    #[test]
    fn a_hash_inside_a_justification_is_not_a_comment() {
        let p = Policy::parse("[allowed]\n\"MIT\" = \"see issue #42\"\n").expect("parses");
        assert!(p.allows("MIT"));
    }
}
