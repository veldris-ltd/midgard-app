//! AC-AD-5 — a policy/access change is simulated to show its exact effect before
//! any commit, and the simulated effect equals the committed effect (sim accuracy).
//! `[FR-AD-6]`

use std::collections::BTreeSet;

use admin_sim::{simulate, AccessProbe};
use policy::{
    Action, ClassificationLevel, Clearance, Compartment, DefaultPdp, Effect, Marking,
    PolicyDecisionPoint, PolicySet, Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector,
    Subject, SubjectId,
};

fn roles(rs: &[&str]) -> BTreeSet<Role> {
    rs.iter().map(|r| Role(r.to_string())).collect()
}
fn purposes(ps: &[&str]) -> BTreeSet<Purpose> {
    ps.iter().map(|p| Purpose(p.to_string())).collect()
}
fn compartments(cs: &[&str]) -> BTreeSet<Compartment> {
    cs.iter().map(|c| Compartment(c.to_string())).collect()
}

fn subject(id: &str, role: &str, level: ClassificationLevel, comps: &[&str]) -> Subject {
    Subject {
        id: SubjectId(id.into()),
        roles: roles(&[role]),
        clearance: Clearance {
            level,
            compartments: compartments(comps),
        },
        purposes: purposes(&["ops"]),
    }
}

fn resource(id: &str, level: ClassificationLevel, comps: &[&str]) -> Resource {
    Resource {
        id: ResourceId(id.into()),
        marking: Marking {
            level,
            compartments: compartments(comps),
        },
        allowed_purposes: purposes(&["ops"]),
    }
}

fn permit_read(rule_id: &str, role: &str) -> Rule {
    Rule {
        id: RuleId(rule_id.into()),
        role: Role(role.into()),
        action: Action("read".into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    }
}

fn probe(label: &str, subj: Subject, res: Resource) -> AccessProbe {
    AccessProbe::new(
        label,
        subj,
        res,
        Action("read".into()),
        Purpose("ops".into()),
    )
}

#[test]
fn ac_ad_5_simulation_shows_grants_before_commit() {
    // Current policy: empty → denies everything. Proposed: permit `reader` to read.
    let current = PolicySet::new(vec![]).unwrap();
    let proposed = PolicySet::new(vec![permit_read("r1", "reader")]).unwrap();

    let probes = vec![
        probe(
            "reader reads open report",
            subject("alice", "reader", ClassificationLevel::UNCLASSIFIED, &[]),
            resource("open", ClassificationLevel::UNCLASSIFIED, &[]),
        ),
        probe(
            // A subject without the granted role is unaffected.
            "guest reads open report",
            subject("guest", "guest", ClassificationLevel::UNCLASSIFIED, &[]),
            resource("open", ClassificationLevel::UNCLASSIFIED, &[]),
        ),
    ];

    let report = simulate(&current, &proposed, &probes);
    assert_eq!(report.grants, vec!["reader reads open report".to_string()]);
    assert_eq!(
        report.unchanged_deny,
        vec!["guest reads open report".to_string()]
    );
    assert!(report.revokes.is_empty());
    assert!(!report.is_no_op());
}

#[test]
fn ac_ad_5_simulated_effect_equals_committed_effect() {
    // The accuracy guarantee: for every probe, what the simulation predicts is
    // exactly what the committed policy actually decides.
    let current = PolicySet::new(vec![permit_read("r1", "reader")]).unwrap();
    // Proposed change: revoke reader (remove the rule) and grant `analyst`.
    let proposed = PolicySet::new(vec![permit_read("r2", "analyst")]).unwrap();

    let probes = vec![
        probe(
            "reader loses access",
            subject("alice", "reader", ClassificationLevel::UNCLASSIFIED, &[]),
            resource("open", ClassificationLevel::UNCLASSIFIED, &[]),
        ),
        probe(
            "analyst gains access",
            subject("bob", "analyst", ClassificationLevel::UNCLASSIFIED, &[]),
            resource("open", ClassificationLevel::UNCLASSIFIED, &[]),
        ),
    ];

    let report = simulate(&current, &proposed, &probes);
    assert_eq!(report.revokes, vec!["reader loses access".to_string()]);
    assert_eq!(report.grants, vec!["analyst gains access".to_string()]);

    // Now "commit" the proposed policy and evaluate the SAME probes for real. Each
    // grant must actually permit; each revoke must actually deny — the simulation
    // was exact.
    let live = DefaultPdp::new(proposed.clone());
    let before = DefaultPdp::new(current.clone());
    for p in &probes {
        let req = p.as_request();
        let live_permits = live.decide(&req).is_permit();
        let before_permits = before.decide(&req).is_permit();
        if report.grants.contains(&p.label) {
            assert!(
                live_permits && !before_permits,
                "grant must flip deny→permit"
            );
        }
        if report.revokes.contains(&p.label) {
            assert!(
                !live_permits && before_permits,
                "revoke must flip permit→deny"
            );
        }
    }
}

#[test]
fn ac_ad_5_marked_data_grant_is_flagged_high_blast_radius() {
    let current = PolicySet::new(vec![]).unwrap();
    let proposed = PolicySet::new(vec![permit_read("r1", "analyst")]).unwrap();

    let probes = vec![probe(
        "analyst reads SECRET report",
        subject("bob", "analyst", ClassificationLevel::SECRET, &["ALPHA"]),
        resource("secret-doc", ClassificationLevel::SECRET, &["ALPHA"]),
    )];

    let report = simulate(&current, &proposed, &probes);
    assert_eq!(report.grants.len(), 1);
    assert_eq!(report.marked_grants.len(), 1);
    assert!(report.high_blast_radius());
    assert!(report.summary().contains("MARKED"));
}

#[test]
fn ac_ad_5_no_op_change_reports_no_effect() {
    let p = PolicySet::new(vec![permit_read("r1", "reader")]).unwrap();
    let probes = vec![probe(
        "reader reads open report",
        subject("alice", "reader", ClassificationLevel::UNCLASSIFIED, &[]),
        resource("open", ClassificationLevel::UNCLASSIFIED, &[]),
    )];
    // Simulating a policy against itself: nothing flips.
    let report = simulate(&p, &p, &probes);
    assert!(report.is_no_op());
    assert!(!report.high_blast_radius());
    assert_eq!(report.unchanged_permit.len(), 1);
}
