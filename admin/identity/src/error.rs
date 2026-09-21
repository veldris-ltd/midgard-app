//! Identity errors. Every variant is a fail-closed denial.

/// A failure in the RBAC / session layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// Durable-store I/O or a malformed on-disk record. Fails closed. [RC-14]
    Io(String),
    /// Sign-in did not present multiple authentication factors. `[AC-AD-1]`
    MfaRequired,
    /// The admin has not completed step-up re-authentication for this scope.
    /// `[AC-AD-1]`
    StepUpRequired {
        /// The scope that requires step-up, rendered `subsystem/selector`.
        scope: String,
    },
    /// A step-up was performed but is older than the permitted freshness window.
    StepUpStale {
        /// The scope whose step-up went stale.
        scope: String,
    },
    /// The presented factor is not strong enough for step-up (a phishing-resistant
    /// factor is required). `[STD:OWASP ASVS V2]`
    WeakStepUpFactor,
    /// The WebAuthn assertion failed verification — wrong credential, forged/absent
    /// signature, or wrong challenge. No session is authenticated. [RC-30]
    WebAuthnRejected,
    /// No such registered admin. [RC-30]
    UnknownAdmin,
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdentityError::Io(m) => write!(f, "durable identity store i/o: {m}"),
            IdentityError::MfaRequired => write!(f, "multi-factor authentication required"),
            IdentityError::StepUpRequired { scope } => {
                write!(f, "step-up re-authentication required for scope {scope}")
            }
            IdentityError::StepUpStale { scope } => {
                write!(f, "step-up for scope {scope} is stale; re-authenticate")
            }
            IdentityError::WeakStepUpFactor => {
                write!(f, "step-up requires a phishing-resistant factor")
            }
            IdentityError::WebAuthnRejected => write!(f, "WebAuthn assertion rejected"),
            IdentityError::UnknownAdmin => write!(f, "unknown admin"),
        }
    }
}

impl std::error::Error for IdentityError {}
