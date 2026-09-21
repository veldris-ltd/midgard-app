//! The risk → controls matrix. One place maps a risk level to the controls an
//! action must satisfy, so control strength scales with danger automatically and
//! is inspected/tested in isolation. `[FR-AD-5]` `[KDD-2]`
//!
//! | Risk | step-up | fresh re-auth | dual-control | break-glass / alert / review |
//! |------|---------|---------------|--------------|------------------------------|
//! | 1    |         |               |              |                              |
//! | 2    | ✔       |               |              |                              |
//! | 3    | ✔       | ✔             |              |                              |
//! | 4    | ✔       | ✔             | ✔            |                              |
//! | 5    | ✔       | ✔             | ✔            | ✔                            |
//!
//! Policy simulation and time-boxed single-use authorisation tokens are enforced
//! by the action orchestrator in a later phase; the booleans here are the
//! authorisation controls this crate owns.

use crate::risk::RiskLevel;

/// The controls required for an action at a given risk level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequiredControls {
    /// The requester must have stepped up into the action's scope (risk ≥ 2).
    pub step_up: bool,
    /// A fresh re-authentication is required at the decision point (risk ≥ 3).
    pub fresh_reauth: bool,
    /// A second, distinct approver is required (risk ≥ 4). `[FR-AD-5]`
    pub dual_control: bool,
    /// The action is break-glass eligible (risk 5). `[FR-AD-8]`
    pub break_glass_eligible: bool,
    /// A real-time alert must be raised (risk 5). `[FR-AD-8]`
    pub real_time_alert: bool,
    /// A mandatory post-hoc review is forced (risk 5). `[FR-AD-8]`
    pub post_hoc_review: bool,
}

/// Map a risk level to its required controls.
pub fn controls_for(risk: RiskLevel) -> RequiredControls {
    let l = risk.level();
    RequiredControls {
        step_up: l >= 2,
        fresh_reauth: l >= 3,
        dual_control: l >= 4,
        break_glass_eligible: l >= 5,
        real_time_alert: l >= 5,
        post_hoc_review: l >= 5,
    }
}
