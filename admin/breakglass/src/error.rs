//! Break-glass errors. Fail closed.

/// A failure granting or reviewing break-glass elevation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakGlassError {
    /// Durable-store I/O or a malformed on-disk record. Fails closed. [RC-14]
    Io(String),
    /// The mandatory justification was empty. `[FR-AD-8]`
    JustificationRequired,
    /// The requester's authenticated session is not the principal being elevated:
    /// break-glass is requested by the person who will hold it. `[MG-SEC-07]`
    SessionMismatch,
    /// A session did not carry a phishing-resistant (passkey) factor. `[MG-SEC-07]`
    MfaRequired,
    /// The approver is the requester: break-glass needs a second person.
    /// `[MG-SEC-07]`
    SelfApprovalDenied,
    /// The approver's passkey re-authentication is older than the approval window.
    /// `[MG-SEC-07]`
    StaleApproval,
    /// The requester holds no active role binding granting `breakglass.request`
    /// over the grant's scope. `[MG-SEC-07]`
    RequesterNotAuthorised,
    /// The approver holds no active role binding granting `breakglass.approve` (the
    /// security officer's) over the grant's scope. `[MG-SEC-07]`
    ApproverNotAuthorised,
    /// The time-box duration was zero.
    InvalidTtl,
    /// The grant id was reused.
    DuplicateGrant(String),
    /// No such grant.
    UnknownGrant(String),
    /// The grant has already been reviewed.
    AlreadyReviewed(String),
    /// Sealing a break-glass transition into the audit chain failed. [RC-32]
    Audit(String),
}

impl std::fmt::Display for BreakGlassError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BreakGlassError::Io(m) => write!(f, "durable break-glass store i/o: {m}"),
            BreakGlassError::JustificationRequired => {
                write!(f, "break-glass requires a justification")
            }
            BreakGlassError::SessionMismatch => {
                write!(
                    f,
                    "the requester's session is not the principal being elevated"
                )
            }
            BreakGlassError::MfaRequired => {
                write!(f, "break-glass requires passkey-authenticated sessions")
            }
            BreakGlassError::SelfApprovalDenied => {
                write!(f, "break-glass cannot be approved by its requester")
            }
            BreakGlassError::StaleApproval => {
                write!(f, "the approver must re-authenticate with their passkey")
            }
            BreakGlassError::RequesterNotAuthorised => {
                write!(f, "the requester may not request break-glass in this scope")
            }
            BreakGlassError::ApproverNotAuthorised => {
                write!(f, "the approver may not approve break-glass in this scope")
            }
            BreakGlassError::InvalidTtl => write!(f, "break-glass time-box must be non-zero"),
            BreakGlassError::DuplicateGrant(id) => write!(f, "duplicate break-glass grant {id}"),
            BreakGlassError::UnknownGrant(id) => write!(f, "unknown break-glass grant {id}"),
            BreakGlassError::AlreadyReviewed(id) => {
                write!(f, "break-glass grant {id} already reviewed")
            }
            BreakGlassError::Audit(m) => write!(f, "break-glass audit failed: {m}"),
        }
    }
}

impl std::error::Error for BreakGlassError {}
