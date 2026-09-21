//! The inference seam. [`RiskAdvisor`] abstracts "send the assembled A.7 prompt to
//! Mimir, get back its JSON risk assessment". Production bridges to the model server
//! over PQ-mTLS (FR-MI-8); tests use a deterministic [`MockAdvisor`]. Keeping Bifrost
//! on this trait (not a hard Mimir dependency) is what makes release intelligence
//! *optional*: with no advisor, the rollout gate still runs on its hard rules
//! (AC-B-6). [FR-B-6] [DD-6]

use crate::error::RiskError;
use crate::prompt::AssembledPrompt;

/// Turns an assembled A.7 prompt into a raw model JSON assessment.
pub trait RiskAdvisor {
    /// Produce the model's raw JSON completion for `prompt`, or fail closed.
    fn assess(&self, prompt: &AssembledPrompt) -> Result<String, RiskError>;
}

/// A deterministic advisor returning a fixed JSON response — the Mimir stand-in for
/// tests and Evals.
pub struct MockAdvisor {
    response: String,
}

impl MockAdvisor {
    /// An advisor that always returns `response`.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

impl RiskAdvisor for MockAdvisor {
    fn assess(&self, _prompt: &AssembledPrompt) -> Result<String, RiskError> {
        Ok(self.response.clone())
    }
}

/// An advisor whose model call always fails — proves the gate still enforces its
/// hard rules when Mimir is unreachable (rather than defaulting to "low risk").
pub struct UnavailableAdvisor;

impl RiskAdvisor for UnavailableAdvisor {
    fn assess(&self, _prompt: &AssembledPrompt) -> Result<String, RiskError> {
        Err(RiskError::Advisor("model server unreachable".into()))
    }
}
