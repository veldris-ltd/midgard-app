//! # scheduler — zero-downtime rolling upgrades
//!
//! The deterministic rollout state machine behind the compute mesh. It upgrades a
//! set of replicas **one at a time**, health-checking each new instance before
//! moving on, and **automatically rolls back** every already-upgraded replica the
//! moment a health check fails. Throughout, it never takes more than one replica
//! out of service, so the number available never drops below `min_available` —
//! zero-downtime. [NFR-AVA-1] [AC-M-7]
//!
//! This is pure orchestration logic (no Kubernetes dependency): a real deployment
//! implements [`HealthCheck`] against live probes and drives the same algorithm.
//!
//! Alongside rollout, [`admission`] provides resource-quota + priority scheduling
//! (SAD_00 §6.2.7): over-quota workloads are throttled into a priority queue and
//! dispatched — or preempted — by priority. [RT-43]

pub mod admission;

pub use admission::{
    Admission, Priority, QuotaScheduler, Resources, TenantId, Workload, WorkloadId,
};

/// A software version identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionId(pub String);

/// A replica identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaId(pub String);

/// Probes whether a replica is healthy while running a given version.
pub trait HealthCheck {
    /// Return `true` iff `replica` is healthy running `version`.
    fn healthy(&self, replica: &ReplicaId, version: &VersionId) -> bool;
}

/// An error from the real orchestrator adapter — a deploy/roll could not even be
/// initiated (the fleet is left untouched; fail closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorError {
    /// Rolling `version` onto `replica` failed to start.
    DeployFailed {
        /// The replica.
        replica: ReplicaId,
        /// Why.
        reason: String,
    },
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrchestratorError::DeployFailed { replica, reason } => {
                write!(f, "deploy to replica {} failed: {reason}", replica.0)
            }
        }
    }
}

impl std::error::Error for OrchestratorError {}

/// The real-orchestrator seam. A production adapter drives a Kubernetes Deployment
/// (surge/maxUnavailable + readiness gates) or the mesh's replica controller:
/// [`deploy`](ReplicaController::deploy) rolls a version onto one replica, and
/// readiness is a **live probe of the running instance**, never an integer counter.
/// The scheduler observes availability by probing the fleet, so "min available" is
/// measured against real, serving replicas. [AC-M-7] [SAD_00 §5/§10]
pub trait ReplicaController {
    /// Roll `version` onto `replica` (start/replace the instance). Returns once the
    /// roll is initiated — readiness is observed separately.
    fn deploy(&self, replica: &ReplicaId, version: &VersionId) -> Result<(), OrchestratorError>;
    /// Block until `replica` passes its live readiness probe, or the adapter's
    /// deploy deadline elapses. `true` iff it became ready. (Readiness-gate timing
    /// lives in the adapter, next to the real probe.)
    fn await_ready(&self, replica: &ReplicaId) -> bool;
    /// Probe whether `replica` is serving **right now** — used to observe live
    /// availability across the fleet during a rollout.
    fn is_ready(&self, replica: &ReplicaId) -> bool;
}

/// The outcome of a rollout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeResult {
    /// Every replica reached the target version and is healthy.
    Upgraded,
    /// A health check failed; every replica was reverted to the source version.
    RolledBack,
}

/// A report on a completed rollout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeReport {
    /// Whether the fleet was upgraded or rolled back.
    pub result: UpgradeResult,
    /// The version every replica is running at the end.
    pub final_version: VersionId,
    /// The fewest replicas available at any instant during the rollout.
    pub min_available_observed: usize,
    /// Whether every replica is healthy at the end.
    pub all_healthy: bool,
}

/// Scheduler errors — fail closed without touching the fleet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerError {
    /// The fleet is too small to roll one replica at a time while keeping
    /// `min_available` in service (need at least `min_available + 1` replicas).
    InsufficientCapacity {
        /// Replicas in the fleet.
        replicas: usize,
        /// Required minimum available.
        min_available: usize,
    },
    /// The rollback target version is itself unhealthy on some replica — the fleet
    /// cannot be brought back to a known-good state automatically.
    RollbackFailed(ReplicaId),
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchedulerError::InsufficientCapacity {
                replicas,
                min_available,
            } => write!(
                f,
                "insufficient capacity: {replicas} replicas cannot keep {min_available} available"
            ),
            SchedulerError::RollbackFailed(r) => {
                write!(f, "rollback target unhealthy on replica {}", r.0)
            }
        }
    }
}

impl std::error::Error for SchedulerError {}

/// A rolling-upgrade scheduler that keeps at least `min_available` replicas up.
pub struct Scheduler {
    min_available: usize,
}

/// Tracks available replicas and the minimum ever observed.
struct Availability {
    current: usize,
    min: usize,
}

impl Availability {
    fn new(total: usize) -> Self {
        Self {
            current: total,
            min: total,
        }
    }
    fn take_down(&mut self) {
        self.current -= 1;
        self.min = self.min.min(self.current);
    }
    fn bring_up(&mut self) {
        self.current += 1;
    }
}

impl Scheduler {
    /// A scheduler that keeps at least `min_available` replicas in service.
    pub fn new(min_available: usize) -> Self {
        Self { min_available }
    }

    /// Roll `replicas` from `from` to `to`, one at a time, health-checking each.
    /// On the first failed health check, revert every already-upgraded replica to
    /// `from` and report `RolledBack`. Availability never falls below
    /// `min_available`. [AC-M-7]
    pub fn rolling_upgrade(
        &self,
        replicas: &[ReplicaId],
        from: &VersionId,
        to: &VersionId,
        health: &dyn HealthCheck,
    ) -> Result<UpgradeReport, SchedulerError> {
        let total = replicas.len();
        // To roll one at a time while keeping `min_available` up, we need at least
        // one spare beyond the minimum.
        if total <= self.min_available {
            return Err(SchedulerError::InsufficientCapacity {
                replicas: total,
                min_available: self.min_available,
            });
        }

        let mut avail = Availability::new(total);
        let mut upgraded: Vec<usize> = Vec::new();

        for i in 0..total {
            // Take this replica out of service and deploy the new version.
            avail.take_down();
            let ok = health.healthy(&replicas[i], to);
            if ok {
                avail.bring_up();
                upgraded.push(i);
                continue;
            }

            // Health check failed — roll everything back to `from`, one at a time.
            // (This replica is already down; revert it first.)
            if !health.healthy(&replicas[i], from) {
                return Err(SchedulerError::RollbackFailed(replicas[i].clone()));
            }
            avail.bring_up();
            for &j in upgraded.iter().rev() {
                avail.take_down();
                if !health.healthy(&replicas[j], from) {
                    return Err(SchedulerError::RollbackFailed(replicas[j].clone()));
                }
                avail.bring_up();
            }
            return Ok(UpgradeReport {
                result: UpgradeResult::RolledBack,
                final_version: from.clone(),
                min_available_observed: avail.min,
                all_healthy: replicas.iter().all(|r| health.healthy(r, from)),
            });
        }

        Ok(UpgradeReport {
            result: UpgradeResult::Upgraded,
            final_version: to.clone(),
            min_available_observed: avail.min,
            all_healthy: replicas.iter().all(|r| health.healthy(r, to)),
        })
    }

    /// Drive a rolling upgrade against a **real** [`ReplicaController`], one replica
    /// at a time, observing availability from **live readiness probes** across the
    /// fleet (not a counter). The reported `min_available_observed` is the fewest
    /// simultaneously-ready replicas measured by probing during the roll. If a new
    /// version never becomes ready (`await_ready` fails), every already-upgraded
    /// replica is reverted to `from` — automatic, health-gated rollback. Never
    /// takes more than one replica out of service, so availability stays ≥
    /// `min_available`. [AC-M-7] [NFR-AVA-1]
    pub fn drive_rolling_upgrade(
        &self,
        replicas: &[ReplicaId],
        from: &VersionId,
        to: &VersionId,
        controller: &dyn ReplicaController,
    ) -> Result<UpgradeReport, SchedulerError> {
        let total = replicas.len();
        if total <= self.min_available {
            return Err(SchedulerError::InsufficientCapacity {
                replicas: total,
                min_available: self.min_available,
            });
        }

        // Observe live availability = number of replicas passing their readiness
        // probe right now. Called at each instant a replica is out of service.
        let observe = |min: &mut usize| {
            let ready = replicas.iter().filter(|r| controller.is_ready(r)).count();
            *min = (*min).min(ready);
        };

        let mut min_observed = usize::MAX;
        observe(&mut min_observed); // baseline sweep
        let mut upgraded: Vec<usize> = Vec::new();

        for i in 0..total {
            // Roll the new version onto this replica; it leaves the ready set until
            // its readiness probe passes.
            controller
                .deploy(&replicas[i], to)
                .map_err(|e| SchedulerError::RollbackFailed(deploy_replica(e)))?;
            observe(&mut min_observed); // measured with replica[i] out of service

            if controller.await_ready(&replicas[i]) {
                upgraded.push(i);
                observe(&mut min_observed);
                continue;
            }

            // New version never became ready — auto-rollback to `from`.
            revert(controller, &replicas[i], from)?;
            for &j in upgraded.iter().rev() {
                controller
                    .deploy(&replicas[j], from)
                    .map_err(|e| SchedulerError::RollbackFailed(deploy_replica(e)))?;
                observe(&mut min_observed);
                revert(controller, &replicas[j], from)?;
            }
            return Ok(UpgradeReport {
                result: UpgradeResult::RolledBack,
                final_version: from.clone(),
                min_available_observed: min_observed,
                all_healthy: replicas.iter().all(|r| controller.is_ready(r)),
            });
        }

        Ok(UpgradeReport {
            result: UpgradeResult::Upgraded,
            final_version: to.clone(),
            min_available_observed: min_observed,
            all_healthy: replicas.iter().all(|r| controller.is_ready(r)),
        })
    }
}

/// Roll `version` onto `replica` and wait for its readiness probe; a target that
/// will not come ready cannot be recovered automatically.
fn revert(
    controller: &dyn ReplicaController,
    replica: &ReplicaId,
    version: &VersionId,
) -> Result<(), SchedulerError> {
    controller
        .deploy(replica, version)
        .map_err(|e| SchedulerError::RollbackFailed(deploy_replica(e)))?;
    if controller.await_ready(replica) {
        Ok(())
    } else {
        Err(SchedulerError::RollbackFailed(replica.clone()))
    }
}

fn deploy_replica(e: OrchestratorError) -> ReplicaId {
    match e {
        OrchestratorError::DeployFailed { replica, .. } => replica,
    }
}
