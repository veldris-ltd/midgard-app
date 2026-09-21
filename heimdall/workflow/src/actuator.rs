//! The **actuator seam**: where an authorised workflow action becomes a real
//! effect. Actuation is the execution half of Heimdall's human-gated action
//! workflow — it runs only after the human authorisation gate and is fully
//! audited. [FR-H-5] [NFR-AI-1] [RT-44] [RC-44]
//!
//! The [`WorkflowEngine`](crate::engine::WorkflowEngine) never actuates on its own
//! and never after only a proposal — actuation runs solely from the `Authorised`
//! state, and every attempt (intent + outcome, or intent + error) is sealed into
//! the PQ-signed audit chain. This module defines the trait the engine invokes and
//! a concrete dispatching actuator a deployment binds to live effectors.
//!
//! A production deployment registers [`Effector`]s that drive real systems — a
//! sensor manager, an asset-dispatch bus, a ticketing queue. The seam keeps the
//! engine's human-gate + audit logic independent of any particular effector, the
//! same way `scheduler::ReplicaController` and `kms::HardwareToken` isolate their
//! side-effecting edges.

use std::collections::BTreeMap;

use crate::action::ProposedAction;

/// Whether the effector carried the command out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActuationStatus {
    /// The effector performed the action.
    Succeeded,
    /// The effector ran but rejected/could-not-complete the command (a real,
    /// audited negative result — distinct from being unable to invoke it at all).
    Failed,
}

impl ActuationStatus {
    /// A short label used in the audit outcome string.
    pub fn label(self) -> &'static str {
        match self {
            ActuationStatus::Succeeded => "succeeded",
            ActuationStatus::Failed => "failed",
        }
    }
}

/// The result of invoking a real effector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActuationOutcome {
    /// Whether the command was carried out.
    pub status: ActuationStatus,
    /// Effector-supplied detail (what it did, or why it declined).
    pub detail: String,
}

impl ActuationOutcome {
    /// A success outcome with detail.
    pub fn succeeded(detail: impl Into<String>) -> Self {
        Self {
            status: ActuationStatus::Succeeded,
            detail: detail.into(),
        }
    }
    /// A completed-but-rejected outcome with a reason.
    pub fn failed(detail: impl Into<String>) -> Self {
        Self {
            status: ActuationStatus::Failed,
            detail: detail.into(),
        }
    }
}

/// The actuator could not be *invoked* for this action (as opposed to invoking it
/// and getting a `Failed` outcome). Fails closed — an action the actuator cannot
/// route is never silently treated as done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActuatorError {
    /// No effector is registered for the action's `action_type`.
    Unsupported(String),
    /// The effector layer was reachable but errored before producing an outcome.
    Unavailable(String),
}

impl std::fmt::Display for ActuatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActuatorError::Unsupported(t) => {
                write!(f, "no effector registered for action type '{t}'")
            }
            ActuatorError::Unavailable(m) => write!(f, "effector unavailable: {m}"),
        }
    }
}

impl std::error::Error for ActuatorError {}

/// The seam the engine invokes to actuate an authorised action.
pub trait Actuator {
    /// Carry out `action`. `Ok` means the effector was invoked and produced an
    /// outcome (success or a rejected command); `Err` means it could not be invoked
    /// at all. Called by the engine ONLY after the human gate. [RC-44]
    fn actuate(&mut self, action: &ProposedAction) -> Result<ActuationOutcome, ActuatorError>;
}

/// A single real effector for one `action_type`. A deployment implements this
/// against a live system; the returned `Ok(detail)` is what was done, `Err(reason)`
/// a rejected command.
pub trait Effector {
    /// Perform the action, returning `Ok(detail)` on success or `Err(reason)` if the
    /// command was rejected.
    fn effect(&mut self, action: &ProposedAction) -> Result<String, String>;
}

/// Routes an action to the [`Effector`] registered for its `action_type`. An
/// unregistered type fails closed with [`ActuatorError::Unsupported`] — a
/// consequential action is never silently no-op'd. This is the concrete actuator a
/// deployment wires to its effectors; it is a real dispatcher, not a mock.
#[derive(Default)]
pub struct DispatchActuator {
    effectors: BTreeMap<String, Box<dyn Effector>>,
}

impl DispatchActuator {
    /// An empty dispatcher.
    pub fn new() -> Self {
        Self {
            effectors: BTreeMap::new(),
        }
    }

    /// Register `effector` to handle `action_type`. Chainable.
    pub fn register(
        &mut self,
        action_type: impl Into<String>,
        effector: Box<dyn Effector>,
    ) -> &mut Self {
        self.effectors.insert(action_type.into(), effector);
        self
    }

    /// Action types this dispatcher can handle.
    pub fn supported(&self) -> Vec<String> {
        self.effectors.keys().cloned().collect()
    }
}

impl Actuator for DispatchActuator {
    fn actuate(&mut self, action: &ProposedAction) -> Result<ActuationOutcome, ActuatorError> {
        let effector = self
            .effectors
            .get_mut(&action.action_type)
            .ok_or_else(|| ActuatorError::Unsupported(action.action_type.clone()))?;
        match effector.effect(action) {
            Ok(detail) => Ok(ActuationOutcome::succeeded(detail)),
            Err(reason) => Ok(ActuationOutcome::failed(reason)),
        }
    }
}
