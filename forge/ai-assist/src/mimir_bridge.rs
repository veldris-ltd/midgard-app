//! The **live** [`TransformSuggester`] — Forge's A.6 NL→transform assist bridged to
//! the Mimir inference plane over PQ-mTLS. [FR-MI-8] [RT-45] [RC-45]
//!
//! Production counterpart to `MockSuggester`/`UnavailableSuggester`: assembles the
//! A.6 prompt, sends it to Mimir as a `FastTransform` request, and returns the raw
//! JSON completion the adapter already parses. Any failure surfaces as
//! [`AssistError::Suggest`], so Forge's non-AI features are untouched. [DD-6]

use mimir_bridge::{ClassificationLevel, Inference, InferenceRequest, Marking, WorkloadClass};

use crate::error::AssistError;
use crate::prompt::AssembledPrompt;
use crate::suggester::TransformSuggester;

/// A [`TransformSuggester`] backed by a live Mimir [`Inference`] transport.
pub struct MimirSuggester<I: Inference> {
    inference: I,
    purpose: String,
    max_tokens: u32,
    marking: Marking,
}

impl<I: Inference> MimirSuggester<I> {
    /// A suggester over `inference` (default "operations" purpose, transform-sized budget).
    pub fn new(inference: I) -> Self {
        Self {
            inference,
            purpose: "operations".into(),
            max_tokens: 512,
            marking: Marking {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: Default::default(),
            },
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
    /// Declare the marking of what this product renders into the prompt: the
    /// caller's posture (level and compartments), which bounds everything the
    /// product can show it and so everything the prompt can hold. Mimir refuses a
    /// marking its model is not accredited for. Default: UNCLASSIFIED, no
    /// compartments. [VLD-ANB-MIDGARD-0034]
    pub fn with_marking(mut self, marking: Marking) -> Self {
        self.marking = marking;
        self
    }
}

impl<I: Inference> TransformSuggester for MimirSuggester<I> {
    fn suggest(&self, prompt: &AssembledPrompt) -> Result<String, AssistError> {
        let request = InferenceRequest {
            purpose: self.purpose.clone(),
            system_prompt: prompt.system_text(),
            workload_class: WorkloadClass::FastTransform,
            quant_pref: None,
            backend_override: None,
            max_tokens: self.max_tokens,
            marking: self.marking.clone(),
            prompt: prompt.body_text(),
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
            boundary: mimir_prompts::delimit::Boundary::fixed("b"),
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
        // The system text travels apart from the delimited body. [VLD-ANB-MIDGARD-0060]
        assert_eq!(req.system_prompt, prompt().system_text());
        assert_eq!(req.prompt, prompt().body_text());
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

    // [VLD-ANB-MIDGARD-0034]
    #[test]
    fn the_request_carries_the_declared_marking() {
        let fake = || FakeInference {
            reply: Ok(text_completion("{}")),
            seen: RefCell::new(None),
        };
        // Undeclared, the prompt is marked UNCLASSIFIED with no compartments.
        let bare = MimirSuggester::new(fake());
        bare.suggest(&prompt()).unwrap();
        let seen = bare.inference.seen.borrow();
        let marking = &seen.as_ref().unwrap().marking;
        assert_eq!(marking.level, ClassificationLevel::UNCLASSIFIED);
        assert!(marking.compartments.is_empty());

        let posture = Marking {
            level: ClassificationLevel::SECRET,
            compartments: [mimir_bridge::Compartment("ALPHA".into())]
                .into_iter()
                .collect(),
        };
        let marked = MimirSuggester::new(fake()).with_marking(posture.clone());
        marked.suggest(&prompt()).unwrap();
        let seen = marked.inference.seen.borrow();
        assert_eq!(seen.as_ref().unwrap().marking, posture);
    }
}
