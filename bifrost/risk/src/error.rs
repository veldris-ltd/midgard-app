//! Risk-adapter errors. A missing or malformed advisory never silently becomes a
//! "low risk" — it is an explicit error, and the deterministic gate still runs on
//! its hard rules regardless.

/// A failure obtaining or parsing an A.7 risk assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskError {
    /// The advisor (Mimir bridge or mock) failed to return a completion.
    Advisor(String),
    /// The model response was not valid JSON or did not match the A.7 contract.
    Parse(String),
}

impl std::fmt::Display for RiskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskError::Advisor(m) => write!(f, "risk inference failed: {m}"),
            RiskError::Parse(m) => write!(f, "malformed A.7 response: {m}"),
        }
    }
}

impl std::error::Error for RiskError {}
