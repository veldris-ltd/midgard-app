//! The Tier-1 on-device assistant seam (A.C1) and its typed output contract.
//!
//! Production binds [`OnDeviceRuntime`] to a small quantised model (int4 GGUF-class)
//! running locally; tests use [`MockOnDeviceRuntime`]. The runtime is handed the
//! assembled A.C1 prompt (see [`crate::prompt`]) and returns an [`OnDeviceOutput`]
//! matching [`mimir_prompts::A_C1_DEFAULT_OUTPUT_CONTRACT`]. `[FR-C-6]`

use crate::error::InferenceError;
use crate::request::InferenceRequest;

/// A proposed, non-executed action emitted by the assistant (sync + governance
/// path decides it later). `[NFR-AI-1]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedAction {
    /// The action type.
    pub action_type: String,
    /// Why the assistant proposes it.
    pub rationale: String,
}

/// A structured refusal (policy-denied or ungrounded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// `POLICY_DENIED` or `UNGROUNDED`.
    pub code: String,
    /// Human-readable detail.
    pub detail: Option<String>,
}

/// The A.C1 output contract, typed. The same shape is produced whether served on
/// Tier 1 (here) or Tier 2/3 (the enclosure) — the tier-parity constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnDeviceOutput {
    /// The answer, or `None` if refused/limited.
    pub answer: Option<String>,
    /// Grounding citations (`obj:<id>`).
    pub citations: Vec<String>,
    /// Whether the task needs heavier reasoning than Tier 1 can give.
    pub escalate_to_enclosure: bool,
    /// State-changing proposals (never executed on-device).
    pub proposed_actions: Vec<ProposedAction>,
    /// Whether the answer is limited by being offline.
    pub offline_limited: bool,
    /// A structured refusal, if any.
    pub refusal: Option<Refusal>,
}

impl OnDeviceOutput {
    /// Whether this output is grounded: it either cites evidence or is an explicit
    /// refusal/limited/escalation (i.e. it never asserts an uncited answer).
    pub fn is_grounded(&self) -> bool {
        self.answer.is_none() || !self.citations.is_empty()
    }
}

/// The on-device inference runtime. Given the assembled A.C1 prompt and the
/// request, produce a contract-valid output.
pub trait OnDeviceRuntime {
    /// Run local inference.
    fn infer(
        &self,
        assembled_prompt: &str,
        request: &InferenceRequest,
    ) -> Result<OnDeviceOutput, InferenceError>;
}

/// A deterministic on-device runtime for tests. It honours the A.C1 rules without
/// a model: grounds in the request's local-context ids, escalates when the task
/// needs remote data while offline, and refuses above-clearance requests.
#[derive(Debug, Default, Clone, Copy)]
pub struct MockOnDeviceRuntime;

impl OnDeviceRuntime for MockOnDeviceRuntime {
    fn infer(
        &self,
        _assembled_prompt: &str,
        request: &InferenceRequest,
    ) -> Result<OnDeviceOutput, InferenceError> {
        // Rule 2: never serve above clearance.
        if request.above_clearance {
            return Ok(OnDeviceOutput {
                answer: None,
                citations: vec![],
                escalate_to_enclosure: false,
                proposed_actions: vec![],
                offline_limited: false,
                refusal: Some(Refusal {
                    code: "POLICY_DENIED".into(),
                    detail: Some("request exceeds session clearance".into()),
                }),
            });
        }
        // Rule 1/3: if the task needs data not present locally, say so and escalate
        // rather than inventing it.
        if request.needs_remote_data {
            return Ok(OnDeviceOutput {
                answer: None,
                citations: vec![],
                escalate_to_enclosure: true,
                proposed_actions: vec![],
                offline_limited: true,
                refusal: None,
            });
        }
        // Grounded answer over the local context.
        Ok(OnDeviceOutput {
            answer: Some(deterministic_answer(&request.user_message)),
            citations: request.local_context_ids.clone(),
            escalate_to_enclosure: false,
            proposed_actions: vec![],
            offline_limited: false,
            refusal: None,
        })
    }
}

/// The shared, deterministic answer function used by both the on-device mock and
/// the remote mock, so identical input yields equivalent output across tiers — the
/// basis of the EV-AC-parity test.
pub(crate) fn deterministic_answer(user_message: &str) -> String {
    format!("answer to: {user_message}")
}
