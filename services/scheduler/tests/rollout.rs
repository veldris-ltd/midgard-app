//! AC-M-7: rolling upgrades keep the fleet available (zero downtime) and roll back
//! automatically when a health check fails. [NFR-AVA-1]

use std::collections::BTreeSet;

use scheduler::{HealthCheck, ReplicaId, Scheduler, SchedulerError, UpgradeResult, VersionId};

/// A health check where the target version is unhealthy on a named set of
/// replicas; every other (replica, version) pair is healthy.
struct FailsOn {
    bad_version: VersionId,
    bad_replicas: BTreeSet<String>,
}
impl HealthCheck for FailsOn {
    fn healthy(&self, replica: &ReplicaId, version: &VersionId) -> bool {
        !(*version == self.bad_version && self.bad_replicas.contains(&replica.0))
    }
}

/// Everything is always healthy.
struct AllHealthy;
impl HealthCheck for AllHealthy {
    fn healthy(&self, _r: &ReplicaId, _v: &VersionId) -> bool {
        true
    }
}

fn fleet(n: usize) -> Vec<ReplicaId> {
    (0..n).map(|i| ReplicaId(format!("r{i}"))).collect()
}

#[test]
fn healthy_upgrade_completes_with_no_downtime() {
    let sched = Scheduler::new(2); // keep >= 2 available
    let replicas = fleet(4);
    let v1 = VersionId("v1".into());
    let v2 = VersionId("v2".into());

    let report = sched
        .rolling_upgrade(&replicas, &v1, &v2, &AllHealthy)
        .unwrap();

    assert_eq!(report.result, UpgradeResult::Upgraded);
    assert_eq!(report.final_version, v2);
    assert!(report.all_healthy);
    // One replica down at a time from four → never fewer than three available.
    assert!(report.min_available_observed >= 2);
    assert_eq!(report.min_available_observed, 3);
}

#[test]
fn failed_health_check_triggers_automatic_rollback() {
    let sched = Scheduler::new(2);
    let replicas = fleet(4);
    let v1 = VersionId("v1".into());
    let v2 = VersionId("v2".into());

    // v2 is unhealthy on r2 → the rollout must roll back r0, r1 (already upgraded)
    // and r2 to v1.
    let health = FailsOn {
        bad_version: v2.clone(),
        bad_replicas: ["r2".to_string()].into_iter().collect(),
    };
    let report = sched.rolling_upgrade(&replicas, &v1, &v2, &health).unwrap();

    assert_eq!(report.result, UpgradeResult::RolledBack);
    assert_eq!(report.final_version, v1);
    assert!(
        report.all_healthy,
        "the whole fleet is back on the healthy v1"
    );
    // Even during rollback the fleet stayed available.
    assert!(report.min_available_observed >= 2);
}

#[test]
fn failure_on_first_replica_rolls_back_cleanly() {
    let sched = Scheduler::new(1);
    let replicas = fleet(3);
    let v1 = VersionId("v1".into());
    let v2 = VersionId("v2".into());
    let health = FailsOn {
        bad_version: v2.clone(),
        bad_replicas: ["r0".to_string()].into_iter().collect(),
    };
    let report = sched.rolling_upgrade(&replicas, &v1, &v2, &health).unwrap();
    assert_eq!(report.result, UpgradeResult::RolledBack);
    assert_eq!(report.final_version, v1);
    assert!(report.all_healthy);
}

#[test]
fn refuses_to_roll_a_fleet_too_small_to_stay_available() {
    // Two replicas but two must stay available → cannot take one down.
    let sched = Scheduler::new(2);
    let replicas = fleet(2);
    let err = sched
        .rolling_upgrade(
            &replicas,
            &VersionId("v1".into()),
            &VersionId("v2".into()),
            &AllHealthy,
        )
        .unwrap_err();
    assert!(matches!(err, SchedulerError::InsufficientCapacity { .. }));
}
