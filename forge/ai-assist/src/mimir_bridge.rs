//! The **live** [`TransformSuggester`] — Forge's A.6 NL→transform assist bridged to
//! the Mimir inference plane over PQ-mTLS. [FR-MI-8] [RT-45] [RC-45]
//!
//! Production counterpart to `MockSuggester`/`UnavailableSuggester`: assembles the
//! A.6 prompt, sends it to Mimir as a `FastTransform` request, and returns the raw
//! JSON completion the adapter already parses. Any failure surfaces as
//! [`AssistError::Suggest`], so Forge's non-AI features are untouched. [DD-6]

use mimir_bridge::{Inference, InferenceRequest, WorkloadClass};

use crate::error::AssistError;
use crate::prompt::AssembledPrompt;
use crate::suggester::TransformSuggester;

/// A [`TransformSuggester`] backed by a live Mimir [`Inference`] transport.
pub struct MimirSuggester<I: Inference> {
    inference: I,
    purpose: String,
    max_tokens: u32,
}

impl<I: Inference> MimirSuggester<I> {
    /// A suggester over `inference` (default "operations" purpose, transform-sized budget).
    pub fn new(inference: I) -> Self {
        Self {
            inference,
            purpose: "operations".into(),
            max_tokens: 512,
        }
    }
    /// Override the declared purpose of access.
    pub fn with_purpose(mut self, purpose: impl Into<String>) -> Self {
        self.purpose = purpose.into();
        self
    }
    /// Override the generation budget.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

impl<I: Inference> TransformSuggester for MimirSuggester<I> {
    fn suggest(&self, prompt: &AssembledPrompt) -> Result<String, AssistError> {
        let request = InferenceRequest {
            purpose: self.purpose.clone(),
            workload_class: WorkloadClass::FastTransform,
            quant_pref: None,
            backend_override: None,
            max_tokens: self.max_tokens,
            prompt: prompt.full_text(),
        };
        let completion = self
            .inference
            .infer(request)
            .map_err(|e| AssistError::Suggest(e.to_string()))?;
        Ok(completion.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_bridge::{text_completion, BridgeError, Completion};
    use std::cell::RefCell;

    struct FakeInference {
        reply: Result<Completion, BridgeError>,
        seen: RefCell<Option<InferenceRequest>>,
    }
    impl Inference for FakeInference {
        fn infer(&self, request: InferenceRequest) -> Result<Completion, BridgeError> {
            *self.seen.borrow_mut() = Some(request);
            self.reply.clone()
        }
    }

    fn prompt() -> AssembledPrompt {
        AssembledPrompt {
            system: "A6".into(),
            schema_text: "name: text".into(),
            library_text: "uppercase".into(),
            user_request: "uppercase the name".into(),
        }
    }

    #[test]
    fn live_suggester_returns_model_text_and_routes_fast_transform() {
        let fake = FakeInference {
            reply: Ok(text_completion("{\"ops\":[]}")),
            seen: RefCell::new(None),
        };
        let suggester = MimirSuggester::new(fake);
        assert_eq!(suggester.suggest(&prompt()).unwrap(), "{\"ops\":[]}");
        let seen = suggester.inference.seen.borrow();
        let req = seen.as_ref().unwrap();
        assert_eq!(req.workload_class, WorkloadClass::FastTransform);
        assert!(req.prompt.contains("uppercase the name"));
    }

    #[test]
    fn failure_degrades_fail_closed() {
        let fake = FakeInference {
            reply: Err(BridgeError::Transport("down".into())),
            seen: RefCell::new(None),
        };
        let suggester = MimirSuggester::new(fake);
        assert!(matches!(
            suggester.suggest(&prompt()),
            Err(AssistError::Suggest(_))
        ));
    }
}
