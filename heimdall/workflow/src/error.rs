//! Workflow errors. Execution fails closed: a consequential action can never run
//! from an unauthorised state, and a denied or already-executed action cannot run at
//! all. [FR-H-5]

/// A workflow transition failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowError {
    /// No workflow exists for this ticket id.
    UnknownAction(String),
    /// A workflow with this id already exists.
    DuplicateAction(String),
    /// Authorisation was attempted on an action not awaiting a decision.
    NotPending(String),
    /// A consequential action was executed without human authorisation. [NFR-AI-1]
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
}

impl std::fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkflowError::UnknownAction(id) => write!(f, "unknown action '{id}'"),
            WorkflowError::DuplicateAction(id) => write!(f, "action '{id}' already exists"),
            WorkflowError::NotPending(id) => {
                write!(f, "action '{id}' is not awaiting authorisation")
            }
            WorkflowError::NotAuthorised(id) => write!(
                f,
                "action '{id}' is consequential and was not authorised by a human"
            ),
            WorkflowError::Denied(id) => write!(f, "action '{id}' was denied"),
            WorkflowError::AlreadyExecuted(id) => write!(f, "action '{id}' already executed"),
            WorkflowError::Audit(m) => write!(f, "audit error: {m}"),
            WorkflowError::Actuation(m) => write!(f, "actuation error: {m}"),
        }
    }
}

impl std::error::Error for WorkflowError {}
