//! The **live** [`OptionProvider`] — Heimdall's A.8 decision support bridged to the
//! Mimir inference plane over PQ-mTLS. [FR-MI-8] [RT-45] [RC-45]
//!
//! This is the production counterpart to `MockOptionProvider`/
//! `UnavailableOptionProvider`: it assembles the A.8 prompt, sends it to Mimir as a
//! `DeepReasoning` [`InferenceRequest`], and returns the raw JSON completion the
//! rest of the pipeline already parses. Any transport or policy failure surfaces as
//! [`DecisionError::Infer`], so decision support degrades exactly as before —
//! fusion/classification/sync are untouched. [DD-6]

use mimir_bridge::{Inference, InferenceRequest, Marking, WorkloadClass};

use crate::error::DecisionError;
use crate::prompt::AssembledPrompt;
use crate::provider::OptionProvider;

/// An [`OptionProvider`] backed by a live Mimir [`Inference`] transport (typically a
/// `mimir_bridge::PqMtlsClient`).
pub struct MimirOptionProvider<I: Inference> {
    inference: I,
    purpose: String,
    max_tokens: u32,
    marking: Marking,
}

impl<I: Inference> MimirOptionProvider<I> {
    /// A provider over `inference`, declaring the "investigation" purpose and a
    /// reasoning-sized token budget by default.
    ///
    /// `marking` is the marking of what this product renders into the prompt: the
    /// caller's posture (level and compartments), which bounds everything the
    /// product can show it and so everything the prompt can hold. It has no
    /// default, so every caller says. Mimir refuses a marking its model is not
    /// accredited for. [VLD-ANB-MIDGARD-0034]
    pub fn new(inference: I, marking: Marking) -> Self {
        Self {
            inference,
            purpose: "investigation".into(),
            max_tokens: 1024,
            marking,
        }
    }

    /// Override the declared purpose of access (audited + policy-checked).
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

impl<I: Inference> OptionProvider for MimirOptionProvider<I> {
    fn generate(&self, prompt: &AssembledPrompt) -> Result<String, DecisionError> {
        let request = InferenceRequest {
            purpose: self.purpose.clone(),
            system_prompt: prompt.system_text(),
            workload_class: WorkloadClass::DeepReasoning,
            quant_pref: None,
            backend_override: None,
            max_tokens: self.max_tokens,
            marking: self.marking.clone(),
            prompt: prompt.body_text(),
        };
        let completion = self
            .inference
            .infer(request)
            .map_err(|e| DecisionError::Infer(e.to_string()))?;
        Ok(completion.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_bridge::{
        text_completion, BridgeError, ClassificationLevel, Compartment, Completion,
    };
    use std::cell::RefCell;

    /// A fake transport capturing the request and returning a canned reply.
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
            system: "A8".into(),
            picture_text: "entity:A123".into(),
            decision_point: "decide".into(),
            boundary: mimir_prompts::delimit::Boundary::fixed("b"),
        }
    }

    /// The caller's posture each bridge is built with.
    fn posture() -> Marking {
        Marking {
            level: ClassificationLevel::SECRET,
            compartments: [Compartment("ALPHA".into())].into_iter().collect(),
        }
    }

    #[test]
    fn live_provider_returns_model_text_and_routes_deep_reasoning() {
        let fake = FakeInference {
            reply: Ok(text_completion("{\"options\":[]}")),
            seen: RefCell::new(None),
        };
        let provider = MimirOptionProvider::new(fake, posture()).with_purpose("operations");
        let out = provider.generate(&prompt()).unwrap();
        assert_eq!(out, "{\"options\":[]}");
        let seen = provider.inference.seen.borrow();
        let req = seen.as_ref().unwrap();
        // The system text travels apart from the delimited body. [VLD-ANB-MIDGARD-0060]
        assert_eq!(req.system_prompt, prompt().system_text());
        assert_eq!(req.prompt, prompt().body_text());
        assert_eq!(req.workload_class, WorkloadClass::DeepReasoning);
        assert_eq!(req.purpose, "operations");
        assert!(
            req.prompt.contains("entity:A123"),
            "the assembled A.8 prompt was sent"
        );
    }

    #[test]
    fn transport_failure_degrades_fail_closed() {
        let fake = FakeInference {
            reply: Err(BridgeError::Denied("policy".into())),
            seen: RefCell::new(None),
        };
        let provider = MimirOptionProvider::new(fake, posture());
        assert!(matches!(
            provider.generate(&prompt()),
            Err(DecisionError::Infer(_))
        ));
    }

    // [VLD-ANB-MIDGARD-0034]
    #[test]
    fn the_declared_marking_is_carried() {
        let fake = FakeInference {
            reply: Ok(text_completion("{}")),
            seen: RefCell::new(None),
        };
        let provider = MimirOptionProvider::new(fake, posture());
        provider.generate(&prompt()).unwrap();
        let seen = provider.inference.seen.borrow();
        assert_eq!(seen.as_ref().unwrap().marking, posture());
    }
}
