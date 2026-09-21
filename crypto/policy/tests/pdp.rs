//! PDP evaluation tests: deny-by-default, MAC-before-discretionary, purpose
//! gating, explicit-deny override, and determinism. [NFR-SEC-2]

use std::collections::BTreeSet;

use policy::{
    AccessRequest, Action, ClassificationLevel, Clearance, Compartment, DecisionReason, DefaultPdp,
    Effect, Marking, PolicyDecisionPoint, PolicySet, Purpose, Resource, ResourceId, Role, Rule,
    RuleId, Selector, Subject, SubjectId,
};

fn set<T: Ord>(items: Vec<T>) -> BTreeSet<T> {
    items.into_iter().collect()
}

fn subject(
    level: ClassificationLevel,
    comps: &[&str],
    roles: &[&str],
    purposes: &[&str],
) -> Subject {
    Subject {
        id: SubjectId("s1".into()),
        roles: set(roles.iter().map(|r| Role(r.to_string())).collect()),
        clearance: Clearance {
            level,
            compartments: set(comps.iter().map(|c| Compartment(c.to_string())).collect()),
        },
        purposes: set(purposes.iter().map(|p| Purpose(p.to_string())).collect()),
    }
}

fn resource(level: ClassificationLevel, comps: &[&str], purposes: &[&str]) -> Resource {
    Resource {
        id: ResourceId("r1".into()),
        marking: Marking {
            level,
            compartments: set(comps.iter().map(|c| Compartment(c.to_string())).collect()),
        },
        allowed_purposes: set(purposes.iter().map(|p| Purpose(p.to_string())).collect()),
    }
}

fn rule(id: &str, role: &str, action: &str, effect: Effect) -> Rule {
    Rule {
        id: RuleId(id.into()),
        role: Role(role.into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect,
    }
}

fn request<'a>(s: &'a Subject, r: &'a Resource, action: &str, purpose: &str) -> AccessRequest<'a> {
    AccessRequest {
        subject: s,
        resource: r,
        action: Action(action.into()),
        purpose: Purpose(purpose.into()),
    }
}

#[test]
fn empty_policy_denies_by_default() {
    let pdp = DefaultPdp::new(PolicySet::new(vec![]).unwrap());
    let s = subject(
        ClassificationLevel::SECRET,
        &[],
        &["analyst"],
        &["investigation"],
    );
    let r = resource(ClassificationLevel::UNCLASSIFIED, &[], &["investigation"]);
    let d = pdp.decide(&request(&s, &r, "read", "investigation"));
    assert_eq!(d.effect, Effect::Deny);
    assert_eq!(d.reasons, vec![DecisionReason::NoMatchingRule]);
}

#[test]
fn permit_via_matching_role() {
    let pdp = DefaultPdp::new(
        PolicySet::new(vec![rule("r-read", "analyst", "read", Effect::Permit)]).unwrap(),
    );
    let s = subject(
        ClassificationLevel::SECRET,
        &[],
        &["analyst"],
        &["investigation"],
    );
    let r = resource(ClassificationLevel::SECRET, &[], &["investigation"]);
    let d = pdp.decide(&request(&s, &r, "read", "investigation"));
    assert!(d.is_permit());
    assert_eq!(
        d.reasons,
        vec![DecisionReason::Permitted {
            rule: RuleId("r-read".into())
        }]
    );
}

#[test]
fn clearance_too_low_denies_even_with_permit_rule() {
    // MAC must be evaluated before discretionary rules: a permit cannot rescue a
    // clearance failure, and the reason names the MAC failure.
    let pdp = DefaultPdp::new(
        PolicySet::new(vec![rule("r-read", "analyst", "read", Effect::Permit)]).unwrap(),
    );
    let s = subject(
        ClassificationLevel::CONFIDENTIAL,
        &[],
        &["analyst"],
        &["investigation"],
    );
    let r = resource(ClassificationLevel::SECRET, &[], &["investigation"]);
    let d = pdp.decide(&request(&s, &r, "read", "investigation"));
    assert_eq!(d.effect, Effect::Deny);
    assert_eq!(
        d.reasons,
        vec![DecisionReason::ClearanceInsufficient {
            needed: ClassificationLevel::SECRET,
            held: ClassificationLevel::CONFIDENTIAL,
        }]
    );
}

#[test]
fn missing_compartment_denies() {
    let pdp = DefaultPdp::new(
        PolicySet::new(vec![rule("r-read", "analyst", "read", Effect::Permit)]).unwrap(),
    );
    let s = subject(
        ClassificationLevel::TOP_SECRET,
        &["ALPHA"],
        &["analyst"],
        &["investigation"],
    );
    let r = resource(
        ClassificationLevel::SECRET,
        &["ALPHA", "BRAVO"],
        &["investigation"],
    );
    let d = pdp.decide(&request(&s, &r, "read", "investigation"));
    assert_eq!(d.effect, Effect::Deny);
    assert_eq!(
        d.reasons,
        vec![DecisionReason::CompartmentMissing {
            missing: vec![Compartment("BRAVO".into())]
        }]
    );
}

#[test]
fn purpose_gate_blocks_even_with_permit_rule() {
    let pdp = DefaultPdp::new(
        PolicySet::new(vec![rule("r-read", "analyst", "read", Effect::Permit)]).unwrap(),
    );
    let s = subject(
        ClassificationLevel::SECRET,
        &[],
        &["analyst"],
        &["training"],
    );
    let r = resource(ClassificationLevel::SECRET, &[], &["investigation"]);
    // Subject's purpose (training) is not permitted for the resource.
    let d = pdp.decide(&request(&s, &r, "read", "training"));
    assert_eq!(d.effect, Effect::Deny);
    assert_eq!(
        d.reasons,
        vec![DecisionReason::PurposeNotPermitted {
            purpose: Purpose("training".into())
        }]
    );
}

#[test]
fn explicit_deny_overrides_permit() {
    let pdp = DefaultPdp::new(
        PolicySet::new(vec![
            rule("r-permit", "analyst", "read", Effect::Permit),
            rule("r-deny", "analyst", "read", Effect::Deny),
        ])
        .unwrap(),
    );
    let s = subject(
        ClassificationLevel::SECRET,
        &[],
        &["analyst"],
        &["investigation"],
    );
    let r = resource(ClassificationLevel::SECRET, &[], &["investigation"]);
    let d = pdp.decide(&request(&s, &r, "read", "investigation"));
    assert_eq!(d.effect, Effect::Deny);
    assert_eq!(
        d.reasons,
        vec![DecisionReason::ExplicitDeny {
            rule: RuleId("r-deny".into())
        }]
    );
}

#[test]
fn decisions_are_deterministic() {
    let pdp = DefaultPdp::new(
        PolicySet::new(vec![rule("r-read", "analyst", "read", Effect::Permit)]).unwrap(),
    );
    let s = subject(
        ClassificationLevel::SECRET,
        &["ALPHA"],
        &["analyst"],
        &["investigation"],
    );
    let r = resource(ClassificationLevel::SECRET, &["ALPHA"], &["investigation"]);
    let d1 = pdp.decide(&request(&s, &r, "read", "investigation"));
    let d2 = pdp.decide(&request(&s, &r, "read", "investigation"));
    assert_eq!(d1, d2);
}

#[test]
fn duplicate_rule_ids_rejected() {
    let err = PolicySet::new(vec![
        rule("dup", "analyst", "read", Effect::Permit),
        rule("dup", "analyst", "write", Effect::Permit),
    ]);
    assert!(err.is_err());
}
