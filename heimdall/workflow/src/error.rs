//! Workflow errors. Execution fails closed: no action can run from an unauthorised
//! state, a denied or already-executed action cannot run at all, and an action type
//! the deployment's catalogue does not list cannot even be proposed. [FR-H-5]
//! [VLD-ANB-MIDGARD-0045]

/// A workflow transition failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowError {
    /// No workflow exists for this ticket id.
    UnknownAction(String),
    /// A workflow with this id already exists.
    DuplicateAction(String),
    /// Authorisation was attempted on an action not awaiting a decision.
    NotPending(String),
    /// An action was executed without a recorded human authorisation. Every
    /// execution needs one, whatever the action's class. [NFR-AI-1]
    NotAuthorised(String),
    /// Execution was attempted on an action a human denied.
    Denied(String),
    /// Execution was attempted on an action that already executed.
    AlreadyExecuted(String),
    /// The audit ledger rejected an entry (carries the ledger's message).
    Audit(String),
    /// The actuator could not be invoked for an authorised action; the attempt was
    /// audited and the action remains authorised (retryable). [RT-44]
    Actuation(String),
    /// The action type is not in the deployment's action-type catalogue; nothing
    /// was audited. [VLD-ANB-MIDGARD-0045]
    UnknownActionType(String),
    /// An action-type catalogue file could not be parsed (carries the reason).
    Catalogue(String),
}

impl std::fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkflowError::UnknownAction(id) => write!(f, "unknown action '{id}'"),
            WorkflowError::DuplicateAction(id) => write!(f, "action '{id}' already exists"),
            WorkflowError::NotPending(id) => {
                write!(f, "action '{id}' is not awaiting authorisation")
            }
            WorkflowError::NotAuthorised(id) => {
                write!(f, "action '{id}' was not authorised by a human")
            }
            WorkflowError::Denied(id) => write!(f, "action '{id}' was denied"),
            WorkflowError::AlreadyExecuted(id) => write!(f, "action '{id}' already executed"),
            WorkflowError::Audit(m) => write!(f, "audit error: {m}"),
            WorkflowError::Actuation(m) => write!(f, "actuation error: {m}"),
            WorkflowError::UnknownActionType(t) => {
                write!(f, "action type '{t}' is not in the action-type catalogue")
            }
            WorkflowError::Catalogue(m) => write!(f, "action-type catalogue error: {m}"),
        }
    }
}

impl std::error::Error for WorkflowError {}
