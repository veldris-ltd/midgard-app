//! The **live** [`OptionProvider`] — Heimdall's A.8 decision support bridged to the
//! Mimir inference plane over PQ-mTLS. [FR-MI-8] [RT-45] [RC-45]
//!
//! This is the production counterpart to `MockOptionProvider`/
//! `UnavailableOptionProvider`: it assembles the A.8 prompt, sends it to Mimir as a
//! `DeepReasoning` [`InferenceRequest`], and returns the raw JSON completion the
//! rest of the pipeline already parses. Any transport or policy failure surfaces as
//! [`DecisionError::Infer`], so decision support degrades exactly as before —
//! fusion/classification/sync are untouched. [DD-6]

use mimir_bridge::{Inference, InferenceRequest, WorkloadClass};

use crate::error::DecisionError;
use crate::prompt::AssembledPrompt;
use crate::provider::OptionProvider;

/// The most tokens an answer to a question may take. The reply is A.1 JSON: an answer
/// citing many items ran past 450 and was cut off mid-string (a parse error after the
/// whole wait), so the cap leaves room for that; most answers stop well short of it.
pub const ANSWER_MAX_TOKENS: u32 = 768;

/// An [`OptionProvider`] backed by a live Mimir [`Inference`] transport (typically a
/// `mimir_bridge::PqMtlsClient`).
pub struct MimirOptionProvider<I: Inference> {
    inference: I,
    purpose: String,
    max_tokens: u32,
}

impl<I: Inference> MimirOptionProvider<I> {
    /// A provider over `inference`, declaring the "investigation" purpose and a
    /// reasoning-sized token budget by default.
    pub fn new(inference: I) -> Self {
        Self {
            inference,
            purpose: "investigation".into(),
            max_tokens: 1024,
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

impl<I: Inference> MimirOptionProvider<I> {
    fn call(
        &self,
        workload_class: WorkloadClass,
        max_tokens: u32,
        prompt: String,
        label: Option<&str>,
    ) -> Result<String, DecisionError> {
        let request = InferenceRequest {
            purpose: self.purpose.clone(),
            workload_class,
            quant_pref: None,
            backend_override: None,
            max_tokens,
            prompt,
            context: label.map(str::to_string),
        };
        self.inference
            .infer(request)
            .map(|c| c.text)
            .map_err(|e| DecisionError::Infer(e.to_string()))
    }
}

impl<I: Inference> OptionProvider for MimirOptionProvider<I> {
    fn generate(&self, prompt: &AssembledPrompt) -> Result<String, DecisionError> {
        self.call(
            WorkloadClass::DeepReasoning,
            self.max_tokens,
            prompt.full_text(),
            None,
        )
    }

    // An answer is shorter than an option set: keep it quick.
    fn complete(&self, prompt: &str) -> Result<String, DecisionError> {
        self.call(
            WorkloadClass::Balanced,
            self.max_tokens.min(ANSWER_MAX_TOKENS),
            prompt.to_string(),
            None,
        )
    }

    fn generate_labelled(
        &self,
        prompt: &AssembledPrompt,
        label: &str,
    ) -> Result<String, DecisionError> {
        self.call(
            WorkloadClass::DeepReasoning,
            self.max_tokens,
            prompt.full_text(),
            Some(label),
        )
    }

    fn complete_labelled(&self, prompt: &str, label: &str) -> Result<String, DecisionError> {
        self.call(
            WorkloadClass::Balanced,
            self.max_tokens.min(ANSWER_MAX_TOKENS),
            prompt.to_string(),
            Some(label),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_bridge::{text_completion, BridgeError, Completion};
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
        }
    }

    #[test]
    fn live_provider_returns_model_text_and_routes_deep_reasoning() {
        let fake = FakeInference {
            reply: Ok(text_completion("{\"options\":[]}")),
            seen: RefCell::new(None),
        };
        let provider = MimirOptionProvider::new(fake).with_purpose("operations");
        let out = provider.generate(&prompt()).unwrap();
        assert_eq!(out, "{\"options\":[]}");
        let seen = provider.inference.seen.borrow();
        let req = seen.as_ref().unwrap();
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
        let provider = MimirOptionProvider::new(fake);
        assert!(matches!(
            provider.generate(&prompt()),
            Err(DecisionError::Infer(_))
        ));
    }
}
