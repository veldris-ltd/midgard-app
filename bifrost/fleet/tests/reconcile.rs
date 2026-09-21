//! Fleet: declarative channels, drift detection, and reconcile-plan generation.
//! [FR-B-3]

use bifrost_fleet::{Fleet, Installation};
use bifrost_rollout::VersionId;

fn fleet() -> Fleet {
    let mut f = Fleet::new();
    f.set_channel("stable", VersionId::new("1.1.0"));
    f.set_channel("edge", VersionId::new("2.0.0-rc1"));
    // Two stable installs behind, one already current; one edge install behind.
    f.register(Installation::new(
        "node-a",
        "stable",
        VersionId::new("1.0.0"),
    ));
    f.register(Installation::new(
        "node-b",
        "stable",
        VersionId::new("1.0.0"),
    ));
    f.register(Installation::new(
        "node-c",
        "stable",
        VersionId::new("1.1.0"),
    ));
    f.register(Installation::new("node-d", "edge", VersionId::new("1.9.0")));
    f
}

#[test]
fn drift_reports_installations_behind_their_channel() {
    let f = fleet();
    let drift = f.drift();
    // node-a, node-b (stable behind) and node-d (edge behind) — not node-c.
    assert_eq!(drift.len(), 3);
    assert!(drift.iter().all(|d| d.install_id != "node-c"));
    assert!(!f.is_converged());

    let a = drift.iter().find(|d| d.install_id == "node-a").unwrap();
    assert_eq!(a.from, VersionId::new("1.0.0"));
    assert_eq!(a.to, VersionId::new("1.1.0"));
}

#[test]
fn reconcile_plan_covers_only_the_drifted_installs_of_a_channel() {
    let f = fleet();
    // 2 stable installs drifted → canary(1) + broad(1).
    let plan = f.reconcile_plan("stable", 1);
    assert_eq!(plan.total_installs(), 2);
    assert_eq!(plan.rings[0].name, "canary");
    assert_eq!(plan.rings[0].installs, 1);
    assert_eq!(plan.rings[1].installs, 1);

    // edge has 1 drifted → a single canary ring (broad omitted).
    let edge = f.reconcile_plan("edge", 1);
    assert_eq!(edge.total_installs(), 1);
    assert_eq!(edge.rings.len(), 1);
}

#[test]
fn updating_installs_converges_the_fleet() {
    let mut f = fleet();
    f.mark_updated("node-a", VersionId::new("1.1.0"));
    f.mark_updated("node-b", VersionId::new("1.1.0"));
    f.mark_updated("node-d", VersionId::new("2.0.0-rc1"));
    assert!(f.is_converged());
    assert!(f.reconcile_plan("stable", 1).rings.is_empty());
}

#[test]
fn install_on_undeclared_channel_is_not_drifted() {
    let mut f = Fleet::new();
    f.register(Installation::new(
        "x",
        "no-such-channel",
        VersionId::new("0.1.0"),
    ));
    assert!(f.drift().is_empty()); // nothing to converge to
    assert!(f.is_converged());
}
