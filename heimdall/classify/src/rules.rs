//! The deterministic rules engine. An ordered list of [`Rule`]s is evaluated
//! against a tracked entity; the first whose [`Condition`] matches yields the
//! label, tagged [`ClassificationSource::Rules`] with the rule's rationale. No
//! model, no I/O — the Mimir-absent, air-gapped path. [FR-H-2] [AC-H-6] [AC-H-7]

use heimdall_picture::{Attr, Classification, ClassificationSource, TrackedEntity};

use crate::Classifier;

/// A boolean condition over an entity's kind and attributes. Composable and total:
/// every condition evaluates to a definite `true`/`false` with no side effects.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// The entity kind equals this string.
    KindIs(String),
    /// A numeric attribute (`Int` or `Float`) is present and strictly greater than
    /// the threshold.
    AttrGreaterThan(String, f64),
    /// A numeric attribute is present and strictly less than the threshold.
    AttrLessThan(String, f64),
    /// An attribute is present and equal to this value.
    AttrEquals(String, Attr),
    /// An attribute is present (any value).
    HasAttr(String),
    /// Both sub-conditions hold.
    And(Box<Condition>, Box<Condition>),
    /// Either sub-condition holds.
    Or(Box<Condition>, Box<Condition>),
    /// The sub-condition does not hold.
    Not(Box<Condition>),
}

impl Condition {
    /// Evaluate against an entity.
    pub fn eval(&self, entity: &TrackedEntity) -> bool {
        match self {
            Condition::KindIs(k) => &entity.kind == k,
            Condition::AttrGreaterThan(name, t) => {
                attr_number(entity, name).map(|v| v > *t).unwrap_or(false)
            }
            Condition::AttrLessThan(name, t) => {
                attr_number(entity, name).map(|v| v < *t).unwrap_or(false)
            }
            Condition::AttrEquals(name, want) => entity
                .latest_attrs
                .get(name)
                .map(|v| v == want)
                .unwrap_or(false),
            Condition::HasAttr(name) => entity.latest_attrs.contains_key(name),
            Condition::And(a, b) => a.eval(entity) && b.eval(entity),
            Condition::Or(a, b) => a.eval(entity) || b.eval(entity),
            Condition::Not(c) => !c.eval(entity),
        }
    }
}

/// Read a numeric attribute as `f64`; non-numeric or absent → `None`.
fn attr_number(entity: &TrackedEntity, name: &str) -> Option<f64> {
    match entity.latest_attrs.get(name) {
        Some(Attr::Int(i)) => Some(*i as f64),
        Some(Attr::Float(f)) => Some(*f),
        _ => None,
    }
}

/// A classification rule: a condition, the label it applies, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// The label applied when `condition` matches.
    pub label: String,
    /// Human-readable rationale recorded on the classification.
    pub rationale: String,
    /// The matching condition.
    pub condition: Condition,
}

impl Rule {
    /// Construct a rule.
    pub fn new(
        label: impl Into<String>,
        rationale: impl Into<String>,
        condition: Condition,
    ) -> Self {
        Self {
            label: label.into(),
            rationale: rationale.into(),
            condition,
        }
    }
}

/// An ordered rules classifier. First matching rule wins; deterministic. [AC-H-6]
#[derive(Debug, Clone, Default)]
pub struct RulesClassifier {
    rules: Vec<Rule>,
}

impl RulesClassifier {
    /// An empty classifier (classifies nothing until rules are added).
    pub fn new() -> Self {
        Self::default()
    }

    /// A small default rule set illustrating common operational labels. Rules are
    /// evaluated in order; earlier rules take precedence.
    pub fn with_default_rules() -> Self {
        Self::new()
            .rule(Rule::new(
                "hostile",
                "explicitly flagged hostile by a feed",
                Condition::AttrEquals("hostile".into(), Attr::Bool(true)),
            ))
            .rule(Rule::new(
                "fast-mover",
                "speed exceeds 400 units",
                Condition::AttrGreaterThan("speed".into(), 400.0),
            ))
            .rule(Rule::new(
                "surface-contact",
                "sea-kind track",
                Condition::KindIs("sea-track".into()),
            ))
            .rule(Rule::new(
                "loiter",
                "speed below 5 units",
                Condition::AttrLessThan("speed".into(), 5.0),
            ))
    }

    /// Append a rule (builder).
    pub fn rule(mut self, rule: Rule) -> Self {
        self.rules.push(rule);
        self
    }

    /// The rules, in evaluation order.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
}

impl Classifier for RulesClassifier {
    fn classify(&self, entity: &TrackedEntity) -> Option<Classification> {
        self.rules
            .iter()
            .find(|r| r.condition.eval(entity))
            .map(|r| Classification {
                label: r.label.clone(),
                source: ClassificationSource::Rules,
                rationale: r.rationale.clone(),
            })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use heimdall_picture::{EntityId, LayerId, Track};

    use super::*;

    fn entity(kind: &str, attrs: &[(&str, Attr)]) -> TrackedEntity {
        let mut map = BTreeMap::new();
        for (k, v) in attrs {
            map.insert((*k).to_string(), v.clone());
        }
        TrackedEntity {
            id: EntityId("e1".into()),
            entity_key: "k1".into(),
            kind: kind.into(),
            layer: LayerId("air".into()),
            sources: std::collections::BTreeSet::new(),
            track: Track::opened_at(1),
            classification: None,
            latest_attrs: map,
        }
    }

    #[test]
    fn first_matching_rule_wins() {
        let c = RulesClassifier::with_default_rules();
        // Both hostile and fast — hostile is listed first.
        let e = entity(
            "air-track",
            &[("hostile", Attr::Bool(true)), ("speed", Attr::Float(900.0))],
        );
        let out = c.classify(&e).expect("classified");
        assert_eq!(out.label, "hostile");
        assert_eq!(out.source, ClassificationSource::Rules);
    }

    #[test]
    fn numeric_threshold_matches() {
        let c = RulesClassifier::with_default_rules();
        let e = entity("air-track", &[("speed", Attr::Int(500))]);
        assert_eq!(c.classify(&e).unwrap().label, "fast-mover");
    }

    #[test]
    fn no_rule_matches_is_none() {
        let c = RulesClassifier::with_default_rules();
        let e = entity("air-track", &[("speed", Attr::Float(100.0))]);
        assert!(c.classify(&e).is_none());
    }

    #[test]
    fn deterministic_same_input_same_label() {
        let c = RulesClassifier::with_default_rules();
        let e = entity("sea-track", &[]);
        assert_eq!(c.classify(&e), c.classify(&e));
        assert_eq!(c.classify(&e).unwrap().label, "surface-contact");
    }
}
