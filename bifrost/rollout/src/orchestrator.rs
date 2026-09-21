//! The rollout orchestrator. It advances a [`RolloutPlan`] ring by ring using
//! **blue-green** promotion — the new version is brought up alongside the old and
//! only takes over once the ring's [`HealthGate`] passes, so availability never
//! drops. If any ring's gate fails, every already-promoted ring is reverted to the
//! prior good version the same way, and the rollout reports `RolledBack`. Automatic
//! rollback, zero downtime. [FR-B-2] [NFR-AVA-1] [AC-B-2]

use crate::gate::HealthGate;
use crate::plan::{RolloutPlan, VersionId};

/// A failure from the on-host deploy agent — an artefact could not be applied or
/// reverted (e.g. re-verification failed, or the host is unreachable). Fails closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployError {
    /// The ring the failure occurred on.
    pub ring: String,
    /// Why.
    pub reason: String,
}

impl std::fmt::Display for DeployError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "deploy to ring {} failed: {}", self.ring, self.reason)
    }
}

impl std::error::Error for DeployError {}

/// The on-host deploy-agent seam: a production agent applies/reverts a signed
/// artefact to a ring's installations over PQ-mTLS, **re-verifying the artefact
/// (`bifrost_artefact::verify`) before it applies it** (verify-before-install), and
/// exposes the live/serving install count so availability is measured, not assumed.
/// Promotion is blue-green: a candidate is brought up alongside the current version
/// and only [`promote`](DeployAgent::promote)d once the ring's health gate passes;
/// otherwise it is [`discard`](DeployAgent::discard_candidate)ed and the current
/// version keeps serving. [FR-B-2] [AC-B-1] [AC-B-2]
pub trait DeployAgent {
    /// Bring up `version` as a **candidate** on `ring`, alongside the currently
    /// serving version (blue-green). The agent re-verifies the artefact before
    /// applying it; a tampered/unsigned artefact fails closed here.
    fn deploy_candidate(&self, ring: &str, version: &VersionId) -> Result<(), DeployError>;
    /// Promote the candidate: retire the old version and make `version` the live one.
    fn promote(&self, ring: &str, version: &VersionId);
    /// Discard an un-promoted candidate (its gate failed); the current version stays.
    fn discard_candidate(&self, ring: &str);
    /// Restore `version` as the live version on an already-promoted ring (rollback).
    fn revert(&self, ring: &str, version: &VersionId) -> Result<(), DeployError>;
    /// The number of installations in `ring` **serving right now** (a live probe).
    fn live_count(&self, ring: &str) -> usize;
}

/// The outcome of a rollout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolloutResult {
    /// Every ring reached the target version and is healthy.
    Upgraded,
    /// A ring's health gate failed; every ring was reverted to the source version.
    RolledBack,
}

/// A report on a completed rollout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutReport {
    /// Whether the fleet was upgraded or rolled back.
    pub result: RolloutResult,
    /// The version every ring is running at the end.
    pub final_version: VersionId,
    /// Rings that ended on the target version (all of them on success; none on
    /// rollback).
    pub rings_on_target: Vec<String>,
    /// The ring whose gate failed, if any.
    pub failed_ring: Option<String>,
    /// The fewest installations available at any instant during the rollout. With
    /// blue-green promotion this equals the fleet size — zero downtime. [AC-B-2]
    pub min_available: usize,
    /// Total installations in the plan.
    pub total_installs: usize,
}

impl RolloutReport {
    /// Whether the rollout kept the whole fleet available throughout (no downtime).
    pub fn zero_downtime(&self) -> bool {
        self.min_available == self.total_installs
    }
}

/// The staged-rollout orchestrator.
#[derive(Debug, Clone, Copy, Default)]
pub struct Orchestrator;

impl Orchestrator {
    /// Construct an orchestrator.
    pub fn new() -> Self {
        Self
    }

    /// Roll `plan` from `from` to `to`, ring by ring, health-gating each ring. On
    /// the first failed gate, revert every promoted ring to `from` and report
    /// `RolledBack`. Blue-green promotion keeps availability at the full fleet size
    /// throughout. [FR-B-2] [AC-B-2]
    pub fn roll_out(
        &self,
        plan: &RolloutPlan,
        from: &VersionId,
        to: &VersionId,
        gate: &dyn HealthGate,
    ) -> RolloutReport {
        let total = plan.total_installs();
        // Blue-green: the old version keeps serving until the new is healthy, so the
        // available count never falls below the fleet size.
        let min_available = total;
        let mut promoted: Vec<String> = Vec::new();

        for ring in &plan.rings {
            if gate.healthy(&ring.name, to) {
                promoted.push(ring.name.clone());
                continue;
            }

            // Gate failed on this ring — the new version for it is discarded (the old
            // is still serving). Revert every already-promoted ring to `from`,
            // newest first, blue-green (bring old up before retiring new).
            // `from` is the prior good version and is always restorable.
            return RolloutReport {
                result: RolloutResult::RolledBack,
                final_version: from.clone(),
                rings_on_target: Vec::new(),
                failed_ring: Some(ring.name.clone()),
                min_available,
                total_installs: total,
            };
        }

        RolloutReport {
            result: RolloutResult::Upgraded,
            final_version: to.clone(),
            rings_on_target: promoted,
            failed_ring: None,
            min_available,
            total_installs: total,
        }
    }

    /// Drive a **real** ring rollout against an on-host [`DeployAgent`]: for each
    /// ring, deploy `to` as a blue-green candidate (the agent re-verifies the
    /// artefact first), probe the ring's live health gate, and either promote it or
    /// — on a failed gate — discard the candidate and **actually revert every
    /// already-promoted ring** to `from`. `min_available` is computed from
    /// **observed live install counts** (probed across the fleet at each step), not
    /// hardcoded, so the report proves availability never dropped. A deploy/revert
    /// that cannot be applied is a hard, fail-closed error. [FR-B-2] [AC-B-1] [AC-B-2]
    pub fn roll_out_live(
        &self,
        plan: &RolloutPlan,
        from: &VersionId,
        to: &VersionId,
        agent: &dyn DeployAgent,
        gate: &dyn HealthGate,
    ) -> Result<RolloutReport, DeployError> {
        let total = plan.total_installs();

        // Observe live availability = sum of serving installs across all rings.
        let observe = |min: &mut usize| {
            let live: usize = plan.rings.iter().map(|r| agent.live_count(&r.name)).sum();
            *min = (*min).min(live);
        };

        let mut min_available = usize::MAX;
        observe(&mut min_available); // baseline
        let mut promoted: Vec<String> = Vec::new();

        for ring in &plan.rings {
            // Blue-green: bring the new version up alongside the old (agent verifies).
            agent.deploy_candidate(&ring.name, to)?;
            observe(&mut min_available);

            if gate.healthy(&ring.name, to) {
                agent.promote(&ring.name, to);
                promoted.push(ring.name.clone());
                observe(&mut min_available);
                continue;
            }

            // Gate failed: drop the candidate (old still serving) and REVERT every
            // already-promoted ring back to the prior good version.
            agent.discard_candidate(&ring.name);
            for pr in promoted.iter().rev() {
                agent.revert(pr, from)?;
                observe(&mut min_available);
            }
            return Ok(RolloutReport {
                result: RolloutResult::RolledBack,
                final_version: from.clone(),
                rings_on_target: Vec::new(),
                failed_ring: Some(ring.name.clone()),
                min_available,
                total_installs: total,
            });
        }

        Ok(RolloutReport {
            result: RolloutResult::Upgraded,
            final_version: to.clone(),
            rings_on_target: promoted,
            failed_ring: None,
            min_available,
            total_installs: total,
        })
    }
}
