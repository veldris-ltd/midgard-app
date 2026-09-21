//! Orchestrator errors. Every variant is a fail-closed refusal: a mutating action [FR-AD-5]
//! that does not clear authentication, simulation, dual-control, and a valid
//! single-use token never reaches the config commit. [RC-31]

use admin_config::ConfigError;
use admin_privilege::PrivError;

/// Why a guarded action was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorError {
    /// Authentication / dual-control refused the action (submit or approve).
    Privilege(PrivError),
    /// The config commit itself failed.
    Config(ConfigError),
    /// No commit token matched, or it was issued for a different action.
    InvalidToken,
    /// The commit token is past its time-box.
    TokenExpired,
    /// The commit token was already used (single-use).
    TokenReused,
    /// The action was never simulated — a commit must be preceded by a dry-run.
    NotSimulated,
    /// The action is not authorised (e.g. a high-risk action lacking a distinct
    /// approver) — no commit may proceed.
    NotAuthorised,
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrchestratorError::Privilege(e) => write!(f, "privilege: {e}"),
            OrchestratorError::Config(e) => write!(f, "config commit: {e}"),
            OrchestratorError::InvalidToken => write!(f, "invalid or mismatched commit token"),
            OrchestratorError::TokenExpired => write!(f, "commit token has expired"),
            OrchestratorError::TokenReused => write!(f, "commit token has already been used"),
            OrchestratorError::NotSimulated => {
                write!(
                    f,
                    "action was not simulated; a dry-run is required before commit"
                )
            }
            OrchestratorError::NotAuthorised => {
                write!(f, "action is not authorised (dual-control not satisfied)")
            }
        }
    }
}

impl std::error::Error for OrchestratorError {}

impl From<PrivError> for OrchestratorError {
    fn from(e: PrivError) -> Self {
        OrchestratorError::Privilege(e)
    }
}
impl From<ConfigError> for OrchestratorError {
    fn from(e: ConfigError) -> Self {
        OrchestratorError::Config(e)
    }
}
