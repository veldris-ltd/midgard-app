//! Rules and the rule set. A rule is discretionary (role-based); mandatory
//! marking/clearance is evaluated separately and always first (see `pdp`).

use crate::error::PolicyError;
use crate::model::{AccessRequest, Action, Effect, Purpose, ResourceId, Role};

/// Identifier for a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleId(pub String);

/// Which resources a rule applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    /// Applies to any resource.
    Any,
    /// Applies to one specific resource.
    Resource(ResourceId),
}

/// A discretionary access rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Unique id.
    pub id: RuleId,
    /// The role this rule grants/denies for.
    pub role: Role,
    /// The action covered.
    pub action: Action,
    /// The purpose covered; `None` matches any purpose.
    pub purpose: Option<Purpose>,
    /// The resources covered.
    pub selector: Selector,
    /// Permit or deny.
    pub effect: Effect,
}

/// An ordered set of rules.
#[derive(Debug, Clone, Default)]
pub struct PolicySet {
    rules: Vec<Rule>,
}

impl PolicySet {
    /// Build a policy set, rejecting duplicate rule ids. An empty set is valid
    /// (it denies everything by default).
    pub fn new(rules: Vec<Rule>) -> Result<Self, PolicyError> {
        for (i, r) in rules.iter().enumerate() {
            if rules[..i].iter().any(|other| other.id == r.id) {
                return Err(PolicyError::DuplicateRuleId(r.id.0.clone()));
            }
        }
        Ok(Self { rules })
    }

    /// The rules, in declaration order.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
}

/// Whether a rule matches a request (role held, action, purpose, selector).
pub(crate) fn rule_matches(rule: &Rule, req: &AccessRequest) -> bool {
    req.subject.roles.contains(&rule.role)
        && rule.action == req.action
        && rule.purpose.as_ref().map_or(true, |p| *p == req.purpose)
        && match &rule.selector {
            Selector::Any => true,
            Selector::Resource(id) => *id == req.resource.id,
        }
}
