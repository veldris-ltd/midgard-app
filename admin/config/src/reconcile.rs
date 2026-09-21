//! GitOps-style **config-drift reconciliation** between the admin-declared config
//! (the signed source of truth in [`ConfigStore`](crate::store::ConfigStore)) and
//! the live state of each managed subsystem. [RT-59] [FR-AD-7]
//!
//! The declared [`ConfigState`] is the desired state, keyed `"<subsystem>/<key>"`.
//! Each managed subsystem is reached through the [`Subsystem`] seam (production
//! impls call the subsystem's admin API; tests use an in-memory one). The
//! [`Reconciler`]:
//! - **[`detect`](Reconciler::detect)** — observes every subsystem and reports drift
//!   vs its declared slice (read-only), and
//! - **[`reconcile`](Reconciler::reconcile)** — converges each drifted subsystem to
//!   the declared state, then **re-observes to confirm** it actually converged.
//!
//! It **fails closed**: a subsystem that cannot be observed is never reported as
//! in-sync, and an `apply` that does not bring the subsystem into agreement is a
//! failure — drift is never silently declared resolved.

use std::collections::BTreeMap;

use crate::diff::ConfigDiff;
use crate::state::ConfigState;

/// A failure interacting with a live subsystem during reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileError {
    /// The subsystem's current config could not be read.
    Observe(String),
    /// The desired config could not be applied to the subsystem.
    Apply(String),
}

impl std::fmt::Display for ReconcileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReconcileError::Observe(m) => write!(f, "cannot observe subsystem: {m}"),
            ReconcileError::Apply(m) => write!(f, "cannot apply config to subsystem: {m}"),
        }
    }
}
impl std::error::Error for ReconcileError {}

/// The seam to one admin-managed subsystem. Presentation-free: it reads the
/// subsystem's live config and applies a desired one. A production impl talks to
/// the subsystem's admin API over PQ-mTLS; tests use an in-memory subsystem.
pub trait Subsystem {
    /// The subsystem id — the prefix used to slice the declared config.
    fn id(&self) -> &str;
    /// Read the subsystem's current live config (its keys, unprefixed).
    fn observe(&self) -> Result<ConfigState, ReconcileError>;
    /// Converge the subsystem to `desired`.
    fn apply(&mut self, desired: &ConfigState) -> Result<(), ReconcileError>;
}

/// Per-subsystem drift status from [`Reconciler::detect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftStatus {
    /// Live config matches the declared slice.
    InSync,
    /// Live config differs — the diff is observed→desired (what would change).
    Drifted(ConfigDiff),
    /// The subsystem could not be observed (fail closed: NOT treated as in-sync).
    Unreachable(String),
}

/// One subsystem's entry in a [`DriftReport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsystemDrift {
    /// The subsystem id.
    pub subsystem: String,
    /// Its drift status.
    pub status: DriftStatus,
}

/// A read-only drift report across all managed subsystems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftReport {
    /// Per-subsystem drift, in registration (sorted-id) order.
    pub entries: Vec<SubsystemDrift>,
}

impl DriftReport {
    /// Whether any subsystem is drifted or unreachable (i.e. not confirmed in-sync).
    pub fn has_drift(&self) -> bool {
        self.entries
            .iter()
            .any(|e| !matches!(e.status, DriftStatus::InSync))
    }
    /// Ids of subsystems whose live config drifted from the declared state.
    pub fn drifted_ids(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, DriftStatus::Drifted(_)))
            .map(|e| e.subsystem.as_str())
            .collect()
    }
    /// Ids of subsystems that could not be observed.
    pub fn unreachable_ids(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, DriftStatus::Unreachable(_)))
            .map(|e| e.subsystem.as_str())
            .collect()
    }
}

/// The outcome of one enforcing reconcile pass for a subsystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileResult {
    /// Was already in sync — nothing applied.
    AlreadyInSync,
    /// Was drifted; the declared state was applied and re-observation confirmed
    /// convergence. Carries the drift that was corrected.
    Corrected(ConfigDiff),
    /// Reconciliation failed — apply errored, or the subsystem did not converge
    /// after apply (fail closed: drift is NOT reported resolved).
    Failed(String),
    /// The subsystem could not be observed at all.
    Unreachable(String),
}

/// One subsystem's entry in a [`ReconcileOutcome`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsystemOutcome {
    /// The subsystem id.
    pub subsystem: String,
    /// What happened.
    pub result: ReconcileResult,
}

/// The outcome of one [`Reconciler::reconcile`] pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileOutcome {
    /// Per-subsystem outcome, in sorted-id order.
    pub outcomes: Vec<SubsystemOutcome>,
}

impl ReconcileOutcome {
    /// True iff every subsystem is now confirmed in sync (already or corrected).
    pub fn all_converged(&self) -> bool {
        self.outcomes.iter().all(|o| {
            matches!(
                o.result,
                ReconcileResult::AlreadyInSync | ReconcileResult::Corrected(_)
            )
        })
    }
    /// Ids of subsystems that were corrected this pass.
    pub fn corrected_ids(&self) -> Vec<&str> {
        self.outcomes
            .iter()
            .filter(|o| matches!(o.result, ReconcileResult::Corrected(_)))
            .map(|o| o.subsystem.as_str())
            .collect()
    }
    /// Ids of subsystems that failed to reconcile (needing operator attention).
    pub fn failed_ids(&self) -> Vec<&str> {
        self.outcomes
            .iter()
            .filter(|o| {
                matches!(
                    o.result,
                    ReconcileResult::Failed(_) | ReconcileResult::Unreachable(_)
                )
            })
            .map(|o| o.subsystem.as_str())
            .collect()
    }
}

/// The drift reconciler: the admin-declared config plus a registry of subsystems.
pub struct Reconciler {
    declared: ConfigState,
    subsystems: BTreeMap<String, Box<dyn Subsystem>>,
}

impl Reconciler {
    /// A reconciler over the declared (desired) config. Keys are `"<subsystem>/<key>"`.
    pub fn new(declared: ConfigState) -> Self {
        Self {
            declared,
            subsystems: BTreeMap::new(),
        }
    }

    /// Register a managed subsystem (keyed by its [`Subsystem::id`]).
    pub fn register(&mut self, subsystem: Box<dyn Subsystem>) -> &mut Self {
        self.subsystems
            .insert(subsystem.id().to_string(), subsystem);
        self
    }

    /// The declared slice for `id`: every declared key prefixed `"<id>/"`, with the
    /// prefix stripped — the subsystem's own view of its desired config.
    fn desired_for(&self, id: &str) -> ConfigState {
        let prefix = format!("{id}/");
        let mut slice = ConfigState::new();
        for (k, v) in self.declared.entries() {
            if let Some(sub_key) = k.strip_prefix(&prefix) {
                slice.set(sub_key, v.clone());
            }
        }
        slice
    }

    /// Observe every subsystem and report drift vs its declared slice. Read-only.
    pub fn detect(&self) -> DriftReport {
        let mut entries = Vec::with_capacity(self.subsystems.len());
        for (id, sub) in &self.subsystems {
            let desired = self.desired_for(id);
            let status = match sub.observe() {
                Err(e) => DriftStatus::Unreachable(e.to_string()),
                Ok(actual) => {
                    let diff = ConfigDiff::between(&actual, &desired);
                    if diff.is_empty() {
                        DriftStatus::InSync
                    } else {
                        DriftStatus::Drifted(diff)
                    }
                }
            };
            entries.push(SubsystemDrift {
                subsystem: id.clone(),
                status,
            });
        }
        DriftReport { entries }
    }

    /// Enforce the declared state: converge each drifted subsystem, then **re-observe
    /// to confirm** it matches. Fails closed — an apply that errors, or that does not
    /// bring the subsystem into agreement, is a `Failed`, never a silent success.
    pub fn reconcile(&mut self) -> ReconcileOutcome {
        // Collect ids up front so we don't hold an immutable borrow of `self` across
        // the mutable `desired_for` / subsystem access.
        let ids: Vec<String> = self.subsystems.keys().cloned().collect();
        let mut outcomes = Vec::with_capacity(ids.len());
        for id in ids {
            let desired = self.desired_for(&id);
            let sub = self.subsystems.get_mut(&id).expect("id came from the map");
            let result = match sub.observe() {
                Err(e) => ReconcileResult::Unreachable(e.to_string()),
                Ok(actual) => {
                    let diff = ConfigDiff::between(&actual, &desired);
                    if diff.is_empty() {
                        ReconcileResult::AlreadyInSync
                    } else if let Err(e) = sub.apply(&desired) {
                        ReconcileResult::Failed(e.to_string())
                    } else {
                        // Confirm convergence: re-observe and require agreement.
                        match sub.observe() {
                            Ok(after) if ConfigDiff::between(&after, &desired).is_empty() => {
                                ReconcileResult::Corrected(diff)
                            }
                            Ok(_) => ReconcileResult::Failed(
                                "subsystem did not converge to the declared config after apply"
                                    .into(),
                            ),
                            Err(e) => ReconcileResult::Failed(format!(
                                "re-observe failed after apply: {e}"
                            )),
                        }
                    }
                }
            };
            outcomes.push(SubsystemOutcome {
                subsystem: id,
                result,
            });
        }
        ReconcileOutcome { outcomes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// An in-memory subsystem holding its own live config.
    struct MemSubsystem {
        id: String,
        state: ConfigState,
    }
    impl MemSubsystem {
        fn new(id: &str, state: ConfigState) -> Self {
            Self {
                id: id.into(),
                state,
            }
        }
    }
    impl Subsystem for MemSubsystem {
        fn id(&self) -> &str {
            &self.id
        }
        fn observe(&self) -> Result<ConfigState, ReconcileError> {
            Ok(self.state.clone())
        }
        fn apply(&mut self, desired: &ConfigState) -> Result<(), ReconcileError> {
            self.state = desired.clone();
            Ok(())
        }
    }

    /// A subsystem that cannot be observed (fail-closed path).
    struct DownSubsystem(String);
    impl Subsystem for DownSubsystem {
        fn id(&self) -> &str {
            &self.0
        }
        fn observe(&self) -> Result<ConfigState, ReconcileError> {
            Err(ReconcileError::Observe("connection refused".into()))
        }
        fn apply(&mut self, _: &ConfigState) -> Result<(), ReconcileError> {
            Err(ReconcileError::Apply("connection refused".into()))
        }
    }

    /// A subsystem whose `apply` silently no-ops — it never actually converges, so
    /// reconcile must report Failed (the confirm-after-apply guard).
    struct StubbornSubsystem {
        id: String,
        state: ConfigState,
        apply_calls: Cell<u32>,
    }
    impl Subsystem for StubbornSubsystem {
        fn id(&self) -> &str {
            &self.id
        }
        fn observe(&self) -> Result<ConfigState, ReconcileError> {
            Ok(self.state.clone())
        }
        fn apply(&mut self, _desired: &ConfigState) -> Result<(), ReconcileError> {
            self.apply_calls.set(self.apply_calls.get() + 1);
            Ok(()) // claims success but does NOT change state
        }
    }

    fn declared() -> ConfigState {
        ConfigState::new()
            .with("iam/session_ttl", "3600")
            .with("iam/mfa", "required")
            .with("gateway/max_conns", "1024")
    }

    #[test]
    fn detect_reports_in_sync_and_drift() {
        let mut r = Reconciler::new(declared());
        // iam matches the declared slice; gateway has drifted (512 != 1024).
        r.register(Box::new(MemSubsystem::new(
            "iam",
            ConfigState::new()
                .with("session_ttl", "3600")
                .with("mfa", "required"),
        )));
        r.register(Box::new(MemSubsystem::new(
            "gateway",
            ConfigState::new().with("max_conns", "512"),
        )));

        let report = r.detect();
        assert!(report.has_drift());
        assert_eq!(report.drifted_ids(), vec!["gateway"]);
        // The iam subsystem is in sync.
        let iam = report
            .entries
            .iter()
            .find(|e| e.subsystem == "iam")
            .unwrap();
        assert_eq!(iam.status, DriftStatus::InSync);
    }

    #[test]
    fn reconcile_corrects_drift_and_confirms_convergence() {
        let mut r = Reconciler::new(declared());
        r.register(Box::new(MemSubsystem::new(
            "gateway",
            ConfigState::new().with("max_conns", "512"), // drifted
        )));

        let outcome = r.reconcile();
        assert!(outcome.all_converged());
        assert_eq!(outcome.corrected_ids(), vec!["gateway"]);

        // A second pass is a no-op — the subsystem now matches the declared state.
        let again = r.reconcile();
        assert!(matches!(
            again.outcomes[0].result,
            ReconcileResult::AlreadyInSync
        ));
        assert!(r.detect().drifted_ids().is_empty());
    }

    #[test]
    fn unreachable_subsystem_fails_closed_never_in_sync() {
        let mut r = Reconciler::new(declared());
        r.register(Box::new(DownSubsystem("iam".into())));

        // Detect: unreachable, and has_drift() is true (not silently in-sync).
        let report = r.detect();
        assert!(report.has_drift());
        assert_eq!(report.unreachable_ids(), vec!["iam"]);

        // Reconcile: reported as needing attention, never converged.
        let outcome = r.reconcile();
        assert!(!outcome.all_converged());
        assert_eq!(outcome.failed_ids(), vec!["iam"]);
    }

    #[test]
    fn apply_that_does_not_converge_is_a_failure() {
        let mut r = Reconciler::new(declared());
        r.register(Box::new(StubbornSubsystem {
            id: "gateway".into(),
            state: ConfigState::new().with("max_conns", "512"), // drifted, apply won't fix it
            apply_calls: Cell::new(0),
        }));

        let outcome = r.reconcile();
        // apply "succeeded" but the confirm-after-apply re-observe still shows drift.
        assert!(!outcome.all_converged());
        assert_eq!(outcome.failed_ids(), vec!["gateway"]);
        assert!(
            matches!(&outcome.outcomes[0].result, ReconcileResult::Failed(m) if m.contains("did not converge"))
        );
    }
}
