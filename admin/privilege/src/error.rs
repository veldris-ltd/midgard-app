//! Privilege-service errors. Every variant is a fail-closed denial.

use admin_identity::IdentityError;

/// A failure in risk classification or the dual-control guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivError {
    /// Durable-store I/O or a malformed on-disk record. Fails closed. [RC-14]
    Io(String),
    /// A risk level outside the valid range 1..=5.
    InvalidRisk(u8),
    /// The presented session does not belong to the requester.
    SessionMismatch,
    /// The acting session lacks MFA.
    MfaRequired,
    /// The requester holds no role granting the capability this action requires.
    NotAuthorisedToRequest {
        /// The capability that was required.
        capability: String,
    },
    /// The action's risk exceeds the requester's role risk ceiling.
    RiskCeilingExceeded {
        /// The action's risk level.
        risk: u8,
    },
    /// An action id was reused.
    DuplicateAction(String),
    /// No such action.
    UnknownAction(String),
    /// The action is not awaiting approval.
    NotPending(String),
    /// The requester attempted to approve their own action (separation of duties).
    /// `[FR-AD-5]` `[AC-AD-4]`
    SelfApprovalDenied,
    /// A panel privilege-logic change requires a security-officer approver.
    /// `[AC-AD-13]`
    SecurityOfficerRequired,
    /// The approver holds no role qualifying them to approve this action.
    /// `[FR-AD-5]`
    ApproverNotQualified {
        /// The capability the approver was required to hold.
        capability: String,
    },
    /// The approver's re-authentication is not fresh enough for the decision.
    StaleReauth,
    /// A wrapped identity/session error (MFA, step-up).
    Auth(IdentityError),
    /// The audit ledger rejected the entry.
    Audit(String),
}

impl std::fmt::Display for PrivError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrivError::Io(m) => write!(f, "durable privilege store i/o: {m}"),
            PrivError::InvalidRisk(l) => write!(f, "invalid risk level {l} (must be 1..=5)"),
            PrivError::SessionMismatch => write!(f, "session does not belong to the requester"),
            PrivError::MfaRequired => write!(f, "multi-factor authentication required"),
            PrivError::NotAuthorisedToRequest { capability } => {
                write!(f, "requester lacks capability {capability}")
            }
            PrivError::RiskCeilingExceeded { risk } => {
                write!(f, "action risk {risk} exceeds the requester's role ceiling")
            }
            PrivError::DuplicateAction(id) => write!(f, "duplicate action id {id}"),
            PrivError::UnknownAction(id) => write!(f, "unknown action {id}"),
            PrivError::NotPending(id) => write!(f, "action {id} is not awaiting approval"),
            PrivError::SelfApprovalDenied => {
                write!(f, "separation of duties: an action cannot self-approve")
            }
            PrivError::SecurityOfficerRequired => {
                write!(f, "a security-officer approver is required")
            }
            PrivError::ApproverNotQualified { capability } => {
                write!(f, "approver lacks the qualifying capability {capability}")
            }
            PrivError::StaleReauth => write!(f, "approver re-authentication is not fresh"),
            PrivError::Auth(e) => write!(f, "authentication: {e}"),
            PrivError::Audit(e) => write!(f, "audit: {e}"),
        }
    }
}

impl std::error::Error for PrivError {}

impl From<IdentityError> for PrivError {
    fn from(e: IdentityError) -> Self {
        PrivError::Auth(e)
    }
}
