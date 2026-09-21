//! Rollout: staged canary→broad with health gates and automatic zero-downtime
//! rollback when a ring's gate fails. [FR-B-2] [NFR-AVA-1] [AC-B-2]

use bifrost_rollout::{HealthGate, Orchestrator, RolloutPlan, RolloutResult, VersionId};

/// A gate that reports a specific ring unhealthy on the target version.
struct FailRing {
    ring: &'static str,
    good: &'static str,
}
impl HealthGate for FailRing {
    fn healthy(&self, ring: &str, version: &VersionId) -> bool {
        // The known-good (source) version is always healthy (so rollback succeeds);
        // the target is healthy everywhere except the configured failing ring.
        if version.0 == self.good {
            return true;
        }
        ring != self.ring
    }
}

/// A gate where everything is healthy on the target.
struct AllHealthy;
impl HealthGate for AllHealthy {
    fn healthy(&self, _ring: &str, _version: &VersionId) -> bool {
        true
    }
}

fn plan() -> RolloutPlan {
    RolloutPlan::canary_broad(100, 5) // 5 canary + 95 broad
}

#[test]
fn healthy_rollout_upgrades_all_rings_with_zero_downtime() {
    let report = Orchestrator::new().roll_out(
        &plan(),
        &VersionId::new("1.0.0"),
        &VersionId::new("1.1.0"),
        &AllHealthy,
    );
    assert_eq!(report.result, RolloutResult::Upgraded);
    assert_eq!(report.final_version, VersionId::new("1.1.0"));
    assert_eq!(report.rings_on_target, vec!["canary", "broad"]);
    assert!(report.zero_downtime());
    assert_eq!(report.min_available, 100);
}

#[test]
fn ac_b_2_broad_ring_gate_failure_rolls_back_automatically_no_downtime() {
    // Canary passes, broad fails its gate → automatic rollback to the source
    // version, and the whole fleet stayed available throughout.
    let gate = FailRing {
        ring: "broad",
        good: "1.0.0",
    };
    let report = Orchestrator::new().roll_out(
        &plan(),
        &VersionId::new("1.0.0"),
        &VersionId::new("1.1.0"),
        &gate,
    );

    assert_eq!(report.result, RolloutResult::RolledBack);
    assert_eq!(report.final_version, VersionId::new("1.0.0")); // reverted
    assert_eq!(report.failed_ring.as_deref(), Some("broad"));
    assert!(report.rings_on_target.is_empty());
    assert!(report.zero_downtime(), "rollback must not cause downtime");
    assert_eq!(report.min_available, 100);
}

#[test]
fn canary_gate_failure_stops_before_broad() {
    // The canary itself fails → broad is never touched.
    let gate = FailRing {
        ring: "canary",
        good: "1.0.0",
    };
    let report = Orchestrator::new().roll_out(
        &plan(),
        &VersionId::new("1.0.0"),
        &VersionId::new("1.1.0"),
        &gate,
    );
    assert_eq!(report.result, RolloutResult::RolledBack);
    assert_eq!(report.failed_ring.as_deref(), Some("canary"));
    assert!(report.zero_downtime());
}

#[test]
fn canary_broad_plan_splits_the_fleet() {
    let p = RolloutPlan::canary_broad(100, 5);
    assert_eq!(p.rings.len(), 2);
    assert_eq!(p.rings[0].name, "canary");
    assert_eq!(p.rings[0].installs, 5);
    assert_eq!(p.rings[1].installs, 95);
    assert_eq!(p.total_installs(), 100);
}
