//! The **live** [`RiskAdvisor`] — Bifrost's A.7 release-risk assist bridged to the
//! Mimir inference plane over PQ-mTLS. [FR-MI-8] [RT-45] [RC-45]
//!
//! Production counterpart to `MockAdvisor`/`UnavailableAdvisor`: assembles the A.7
//! prompt, sends it to Mimir as a `CodeAnalysis` request, and returns the raw JSON
//! assessment. Any failure surfaces as [`RiskError::Advisor`], so the rollout gate
//! still runs on its hard rules — never defaulting to "low risk". [AC-B-6] [DD-6]

use mimir_bridge::{Inference, InferenceRequest, WorkloadClass};

use crate::advisor::RiskAdvisor;
use crate::error::RiskError;
use crate::prompt::AssembledPrompt;

/// A [`RiskAdvisor`] backed by a live Mimir [`Inference`] transport.
pub struct MimirAdvisor<I: Inference> {
    inference: I,
    purpose: String,
    max_tokens: u32,
}

impl<I: Inference> MimirAdvisor<I> {
    /// An advisor over `inference` (default "operations" purpose, analysis-sized budget).
    pub fn new(inference: I) -> Self {
        Self {
            inference,
            purpose: "operations".into(),
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

impl<I: Inference> RiskAdvisor for MimirAdvisor<I> {
    fn assess(&self, prompt: &AssembledPrompt) -> Result<String, RiskError> {
        let request = InferenceRequest {
            purpose: self.purpose.clone(),
            workload_class: WorkloadClass::CodeAnalysis,
            quant_pref: None,
            backend_override: None,
            max_tokens: self.max_tokens,
            prompt: prompt.full_text(),
        };
        let completion = self
            .inference
            .infer(request)
            .map_err(|e| RiskError::Advisor(e.to_string()))?;
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
            system: "A7".into(),
            release_context: "cve: none".into(),
        }
    }

    #[test]
    fn live_advisor_returns_model_text_and_routes_code_analysis() {
        let fake = FakeInference {
            reply: Ok(text_completion("{\"band\":\"low\"}")),
            seen: RefCell::new(None),
        };
        let advisor = MimirAdvisor::new(fake);
        assert_eq!(advisor.assess(&prompt()).unwrap(), "{\"band\":\"low\"}");
        let seen = advisor.inference.seen.borrow();
        let req = seen.as_ref().unwrap();
        assert_eq!(req.workload_class, WorkloadClass::CodeAnalysis);
        assert!(req.prompt.contains("cve: none"));
    }

    #[test]
    fn failure_degrades_fail_closed() {
        let fake = FakeInference {
            reply: Err(BridgeError::Malformed("x".into())),
            seen: RefCell::new(None),
        };
        let advisor = MimirAdvisor::new(fake);
        assert!(matches!(
            advisor.assess(&prompt()),
            Err(RiskError::Advisor(_))
        ));
    }
}
