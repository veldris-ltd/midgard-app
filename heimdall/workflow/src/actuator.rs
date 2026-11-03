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
//! The [`DispatchActuator`] also holds the deployment's [`ActionTypeCatalog`] and
//! re-checks every action against it before touching an effector: a type the
//! catalogue does not list, or whose recorded class disagrees with the
//! catalogue's, is refused. [VLD-ANB-MIDGARD-0045]
//!
//! A production deployment registers [`Effector`]s that drive real systems — a
//! sensor manager, an asset-dispatch bus, a ticketing queue. The seam keeps the
//! engine's human-gate + audit logic independent of any particular effector, the
//! same way `scheduler::ReplicaController` and `kms::HardwareToken` isolate their
//! side-effecting edges.

use std::collections::BTreeMap;

use crate::action::ProposedAction;
use crate::catalog::{ActionClass, ActionTypeCatalog};

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
    /// The actuator does not handle the action's `action_type`: its catalogue does
    /// not list the type, or no effector is registered for it. Carries the type.
    Unsupported(String),
    /// The action's recorded class disagrees with the actuator's catalogue (the
    /// record was classified against a different catalogue, or altered).
    /// [VLD-ANB-MIDGARD-0045]
    ClassMismatch {
        /// The action type.
        action_type: String,
        /// The class the actuator's catalogue gives the type.
        catalogued: ActionClass,
        /// The class recorded on the action.
        recorded: ActionClass,
    },
    /// The effector layer was reachable but errored before producing an outcome.
    Unavailable(String),
}

impl std::fmt::Display for ActuatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActuatorError::Unsupported(t) => write!(
                f,
                "action type '{t}' is unsupported: not in the actuator's catalogue, or no effector registered"
            ),
            ActuatorError::ClassMismatch {
                action_type,
                catalogued,
                recorded,
            } => write!(
                f,
                "action type '{action_type}' is {catalogued} in the actuator's catalogue but recorded as {recorded}"
            ),
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

/// Routes an action to the [`Effector`] registered for its `action_type`, after
/// re-checking the type against its [`ActionTypeCatalog`]. A type the catalogue
/// does not list, or that has no registered effector, fails closed with
/// [`ActuatorError::Unsupported`]; a recorded class that disagrees with the
/// catalogue fails with [`ActuatorError::ClassMismatch`] — a consequential action
/// is never silently no-op'd or run under the wrong class. With no catalogue
/// supplied it refuses everything. This is the concrete actuator a deployment
/// wires to its effectors; it is a real dispatcher, not a mock.
#[derive(Default)]
pub struct DispatchActuator {
    effectors: BTreeMap<String, Box<dyn Effector>>,
    catalog: ActionTypeCatalog,
}

impl DispatchActuator {
    /// An empty dispatcher with an empty catalogue (refuses every action).
    pub fn new() -> Self {
        Self {
            effectors: BTreeMap::new(),
            catalog: ActionTypeCatalog::new(),
        }
    }

    /// Use `catalog` — the same catalogue the engine classifies with — to re-check
    /// each action before actuation (builder). [VLD-ANB-MIDGARD-0045]
    pub fn with_catalog(mut self, catalog: ActionTypeCatalog) -> Self {
        self.catalog = catalog;
        self
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
        // Re-check the deployment's catalogue before any effector is touched.
        let catalogued = self
            .catalog
            .class_of(&action.action_type)
            .ok_or_else(|| ActuatorError::Unsupported(action.action_type.clone()))?;
        if catalogued != action.class() {
            return Err(ActuatorError::ClassMismatch {
                action_type: action.action_type.clone(),
                catalogued,
                recorded: action.class(),
            });
        }
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
