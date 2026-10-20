//! The **live** [`CopilotAdvisor`] — the Admin A.AD1 copilot bridged to the Mimir
//! inference plane over PQ-mTLS. [FR-MI-8] [RT-45] [RC-45]
//!
//! Production counterpart to `MockCopilot`/`UnavailableCopilot`: assembles the
//! A.AD1 prompt, sends it to Mimir as a `Balanced` request, and returns the raw JSON
//! draft. Any failure surfaces as [`AdviceError::Advisor`]; the copilot is advisory
//! only, so the admin panel stays usable via the guarded, dual-controlled path when
//! Mimir is absent. [AC-AD-11] [DD-6]

use mimir_bridge::{Inference, InferenceRequest, WorkloadClass};

use crate::copilot::{AssembledCopilotPrompt, CopilotAdvisor};
use crate::error::AdviceError;

/// A [`CopilotAdvisor`] backed by a live Mimir [`Inference`] transport.
pub struct MimirCopilot<I: Inference> {
    inference: I,
    purpose: String,
    max_tokens: u32,
}

impl<I: Inference> MimirCopilot<I> {
    /// A copilot over `inference` (default "administration" purpose, balanced budget).
    pub fn new(inference: I) -> Self {
        Self {
            inference,
            purpose: "administration".into(),
            max_tokens: 1024,
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

impl<I: Inference> CopilotAdvisor for MimirCopilot<I> {
    fn draft(&self, prompt: &AssembledCopilotPrompt) -> Result<String, AdviceError> {
        let request = InferenceRequest {
            purpose: self.purpose.clone(),
            workload_class: WorkloadClass::Balanced,
            quant_pref: None,
            backend_override: None,
            max_tokens: self.max_tokens,
            prompt: prompt.full_text(),
            context: None,
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
        assert_eq!(copilot.draft(&prompt()).unwrap(), "{\"changes\":[]}");
        let seen = copilot.inference.seen.borrow();
        let req = seen.as_ref().unwrap();
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
}
