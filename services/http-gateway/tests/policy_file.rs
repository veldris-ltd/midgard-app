//! The policy file carries exactly the rule set the PDP enforces: the development
//! policy round-trips through it, a hand-written file loads rule for rule, and a
//! file the loader cannot read exactly is refused whole. [NFR-SEC-2] [AC-M-4]

use http_gateway::dev_login_policy;
use http_gateway::policy_file::{parse_policy, render_policy, PolicyFileError};
use policy::{
    Action, Effect, PolicyError, PolicySet, Purpose, ResourceId, Role, Rule, RuleId, Selector,
};

#[test]
fn the_development_policy_round_trips() {
    let dev = dev_login_policy();
    let text = render_policy(&dev).expect("renders");
    let loaded = parse_policy(&text).expect("loads back");
    assert_eq!(loaded.rules(), dev.rules());
    assert!(!loaded.rules().is_empty());
}

#[test]
fn a_hand_written_file_loads_rule_for_rule() {
    let loaded = parse_policy(
        r#"{
          "version": 1,
          "rules": [
            { "id": "operator-read", "role": "operator", "action": "Read",
              "resource": "*", "effect": "permit" },
            { "id": "no-export", "role": "analyst", "action": "Read",
              "purpose": "export", "resource": "object-type:aircraft", "effect": "deny" }
          ]
        }"#,
    )
    .expect("loads");
    assert_eq!(
        loaded.rules(),
        &[
            Rule {
                id: RuleId("operator-read".into()),
                role: Role("operator".into()),
                action: Action("Read".into()),
                purpose: None,
                selector: Selector::Any,
                effect: Effect::Permit,
            },
            Rule {
                id: RuleId("no-export".into()),
                role: Role("analyst".into()),
                action: Action("Read".into()),
                purpose: Some(Purpose("export".into())),
                selector: Selector::Resource(ResourceId("object-type:aircraft".into())),
                effect: Effect::Deny,
            },
        ]
    );
}

#[test]
fn an_empty_rule_set_is_valid_and_grants_nothing() {
    let loaded = parse_policy(r#"{"version":1,"rules":[]}"#).expect("loads");
    assert!(loaded.rules().is_empty());
}

#[test]
fn a_file_that_cannot_be_read_exactly_is_refused() {
    let rule = |fields: &str| format!(r#"{{"version":1,"rules":[{{{fields}}}]}}"#);
    let good = r#""id":"r","role":"operator","action":"Read","resource":"*","effect":"permit""#;
    assert!(parse_policy(&rule(good)).is_ok(), "the baseline rule loads");

    let cases: Vec<(&str, String)> = vec![
        ("not JSON", "{".into()),
        ("not an object", "[]".into()),
        ("missing version", r#"{"rules":[]}"#.into()),
        ("future version", r#"{"version":2,"rules":[]}"#.into()),
        ("rules not an array", r#"{"version":1,"rules":{}}"#.into()),
        (
            "unknown top-level key",
            r#"{"version":1,"rules":[],"default":"permit"}"#.into(),
        ),
        (
            "repeated top-level key",
            r#"{"version":1,"version":1,"rules":[]}"#.into(),
        ),
        (
            "rule not an object",
            r#"{"version":1,"rules":["operator"]}"#.into(),
        ),
        (
            "misspelt key",
            rule(r#""id":"r","role":"operator","action":"Read","resource":"*","efect":"permit""#),
        ),
        (
            "missing role",
            rule(r#""id":"r","action":"Read","resource":"*","effect":"permit""#),
        ),
        (
            "empty action",
            rule(r#""id":"r","role":"operator","action":" ","resource":"*","effect":"permit""#),
        ),
        (
            "non-string role",
            rule(
                r#""id":"r","role":["operator"],"action":"Read","resource":"*","effect":"permit""#,
            ),
        ),
        (
            "null purpose",
            rule(
                r#""id":"r","role":"operator","action":"Read","purpose":null,"resource":"*","effect":"permit""#,
            ),
        ),
        (
            "unknown effect",
            rule(r#""id":"r","role":"operator","action":"Read","resource":"*","effect":"allow""#),
        ),
        (
            "repeated rule key",
            rule(
                r#""id":"r","role":"operator","role":"admin","action":"Read","resource":"*","effect":"permit""#,
            ),
        ),
    ];
    for (label, text) in &cases {
        assert!(
            matches!(
                parse_policy(text),
                Err(PolicyFileError::Syntax(_) | PolicyFileError::Shape(_))
            ),
            "{label} must be refused: {text}"
        );
    }

    let duplicate = format!(r#"{{"version":1,"rules":[{{{good}}},{{{good}}}]}}"#);
    assert_eq!(
        parse_policy(&duplicate).err(),
        Some(PolicyFileError::Policy(PolicyError::DuplicateRuleId(
            "r".into()
        )))
    );
}

#[test]
fn a_rule_for_a_resource_named_star_is_not_written_as_any_resource() {
    let set = PolicySet::new(vec![Rule {
        id: RuleId("literal-star".into()),
        role: Role("operator".into()),
        action: Action("Read".into()),
        purpose: None,
        selector: Selector::Resource(ResourceId("*".into())),
        effect: Effect::Permit,
    }])
    .expect("valid set");
    assert!(matches!(
        render_policy(&set),
        Err(PolicyFileError::Shape(_))
    ));
}
