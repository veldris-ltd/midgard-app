//! The inference seam. [`OptionProvider`] abstracts "send the assembled A.8 prompt
//! to Mimir, get back its JSON option set". Production bridges to the model server
//! over PQ-mTLS (FR-MI-8); tests and Evals use a deterministic [`MockOptionProvider`].
//! Keeping Heimdall on this trait (not a hard Mimir dependency) is what makes
//! decision support optional — with no provider, fusion/classification/sync are
//! untouched. [FR-H-4] [DD-6]

use crate::error::DecisionError;
use crate::prompt::AssembledPrompt;

/// Turns an assembled A.8 prompt into a raw model JSON option set.
pub trait OptionProvider {
    /// Produce the model's raw JSON completion for `prompt`, or fail closed.
    fn generate(&self, prompt: &AssembledPrompt) -> Result<String, DecisionError>;
}

/// A deterministic provider returning a fixed JSON response — the Mimir stand-in for
/// tests and Evals.
pub struct MockOptionProvider {
    response: String,
}

impl MockOptionProvider {
    /// A provider that always returns `response`.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

impl OptionProvider for MockOptionProvider {
    fn generate(&self, _prompt: &AssembledPrompt) -> Result<String, DecisionError> {
        Ok(self.response.clone())
    }
}

/// A provider whose model call always fails — proves the adapter fails closed when
/// Mimir is unreachable (rather than fabricating options). Decision support is then
/// simply unavailable; the rest of Heimdall keeps running. [AC-H-6]
pub struct UnavailableOptionProvider;

impl OptionProvider for UnavailableOptionProvider {
    fn generate(&self, _prompt: &AssembledPrompt) -> Result<String, DecisionError> {
        Err(DecisionError::Infer("model server unreachable".into()))
    }
}
