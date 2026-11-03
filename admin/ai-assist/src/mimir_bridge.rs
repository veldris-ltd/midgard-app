//! The **live** [`CopilotAdvisor`] — the Admin A.AD1 copilot bridged to the Mimir
//! inference plane over PQ-mTLS. [FR-MI-8] [RT-45] [RC-45]
//!
//! Production counterpart to `MockCopilot`/`UnavailableCopilot`: assembles the
//! A.AD1 prompt, sends it to Mimir as a `Balanced` request, and returns the raw JSON
//! draft. Any failure surfaces as [`AdviceError::Advisor`]; the copilot is advisory
//! only, so the admin panel stays usable via the guarded, dual-controlled path when
//! Mimir is absent. [AC-AD-11] [DD-6]

use mimir_bridge::{ClassificationLevel, Inference, InferenceRequest, Marking, WorkloadClass};

use crate::copilot::{AssembledCopilotPrompt, CopilotAdvisor};
use crate::error::AdviceError;

/// A [`CopilotAdvisor`] backed by a live Mimir [`Inference`] transport.
pub struct MimirCopilot<I: Inference> {
    inference: I,
    purpose: String,
    max_tokens: u32,
    marking: Marking,
}

impl<I: Inference> MimirCopilot<I> {
    /// A copilot over `inference` (default "administration" purpose, balanced budget).
    pub fn new(inference: I) -> Self {
        Self {
            inference,
            purpose: "administration".into(),
            max_tokens: 1024,
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

impl<I: Inference> CopilotAdvisor for MimirCopilot<I> {
    fn draft(&self, prompt: &AssembledCopilotPrompt) -> Result<String, AdviceError> {
        let request = InferenceRequest {
            purpose: self.purpose.clone(),
            system_prompt: prompt.system_text(),
            workload_class: WorkloadClass::Balanced,
            quant_pref: None,
            backend_override: None,
            max_tokens: self.max_tokens,
            marking: self.marking.clone(),
            prompt: prompt.body_text(),
        };
        let completion = self
            .inference
            .infer(request)
            .map_err(|e| AdviceError::Advisor(e.to_string()))?;
        Ok(completion.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copilot::{assemble_copilot, CopilotContext};
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

    fn prompt() -> AssembledCopilotPrompt {
        assemble_copilot(&CopilotContext {
            admin_role: "operator".into(),
            state: "cfg".into(),
            policy: "policy".into(),
            output_contract: "{}".into(),
            admin_message: "rotate the signing key".into(),
        })
    }

    #[test]
    fn live_copilot_returns_model_text_and_routes_balanced() {
        let fake = FakeInference {
            reply: Ok(text_completion("{\"changes\":[]}")),
            seen: RefCell::new(None),
        };
        let copilot = MimirCopilot::new(fake);
        let sent = prompt();
        assert_eq!(copilot.draft(&sent).unwrap(), "{\"changes\":[]}");
        let seen = copilot.inference.seen.borrow();
        let req = seen.as_ref().unwrap();
        // The system text travels apart from the delimited body. [VLD-ANB-MIDGARD-0060]
        assert_eq!(req.system_prompt, sent.system_text());
        assert_eq!(req.prompt, sent.body_text());
        assert_eq!(req.workload_class, WorkloadClass::Balanced);
        assert!(req.prompt.contains("rotate the signing key"));
    }

    #[test]
    fn failure_degrades_fail_closed() {
        let fake = FakeInference {
            reply: Err(BridgeError::Denied("no".into())),
            seen: RefCell::new(None),
        };
        let copilot = MimirCopilot::new(fake);
        assert!(matches!(
            copilot.draft(&prompt()),
            Err(AdviceError::Advisor(_))
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
        let bare = MimirCopilot::new(fake());
        bare.draft(&prompt()).unwrap();
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
        let marked = MimirCopilot::new(fake()).with_marking(posture.clone());
        marked.draft(&prompt()).unwrap();
        let seen = marked.inference.seen.borrow();
        assert_eq!(seen.as_ref().unwrap().marking, posture);
    }
}
