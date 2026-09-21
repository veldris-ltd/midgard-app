//! The inference seam. [`TransformSuggester`] abstracts "send the assembled A.6
//! prompt to a model, get back its JSON response". In production a suggester
//! bridges to the Mimir model server over internal PQ-mTLS (FR-MI-8); for tests and
//! Evals a [`MockSuggester`] returns a deterministic response — the same
//! seam-with-deterministic-mock pattern Mimir uses for its GPU backend. Keeping
//! Forge on this trait (not a hard Mimir dependency) is also what makes the whole
//! AI-assist path optional: with no suggester, Forge's non-AI features are
//! untouched. [FR-F-7] [DD-6]

use crate::error::AssistError;
use crate::prompt::AssembledPrompt;

/// Turns an assembled A.6 prompt into a raw model JSON response.
pub trait TransformSuggester {
    /// Produce the model's raw JSON completion for `prompt`, or fail closed.
    fn suggest(&self, prompt: &AssembledPrompt) -> Result<String, AssistError>;
}

/// A deterministic suggester returning a fixed JSON response. Stands in for the
/// Mimir bridge in tests and Evals.
pub struct MockSuggester {
    response: String,
}

impl MockSuggester {
    /// A suggester that always returns `response`.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

impl TransformSuggester for MockSuggester {
    fn suggest(&self, _prompt: &AssembledPrompt) -> Result<String, AssistError> {
        Ok(self.response.clone())
    }
}

/// A suggester whose model call always fails — used to prove the adapter fails
/// closed when Mimir is unreachable (rather than fabricating a transform).
pub struct UnavailableSuggester;

impl TransformSuggester for UnavailableSuggester {
    fn suggest(&self, _prompt: &AssembledPrompt) -> Result<String, AssistError> {
        Err(AssistError::Suggest("model server unreachable".into()))
    }
}
