//! The Tier-2 remote delegation seam. When a request is served remotely the
//! adapter delegates to enclosure Mimir; production bridges this over the client's
//! PQ-mTLS session (`midgard-client-enclosure`), and the adapter records that the
//! result was grounded, cited, and audited. `[FR-C-6]` `[AC-C-6]`

use crate::error::InferenceError;
use crate::ondevice::deterministic_answer;
use crate::request::InferenceRequest;

/// A remote inference result. Must be grounded and cited — the adapter refuses an
/// ungrounded remote answer. `[AC-C-6]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteOutput {
    /// The answer.
    pub answer: String,
    /// Grounding citations (`obj:<id>`) returned by the enclosure OAG.
    pub citations: Vec<String>,
    /// Whether the enclosure marked the result grounded.
    pub grounded: bool,
}

/// Delegates heavier reasoning to enclosure Mimir over PQ-mTLS.
pub trait RemoteInference {
    /// Run remote inference.
    fn infer(&self, request: &InferenceRequest) -> Result<RemoteOutput, InferenceError>;
}

/// A deterministic remote runtime for tests. Returns a grounded, cited answer that
/// matches the on-device mock's answer for the same input (tier parity).
#[derive(Debug, Default, Clone, Copy)]
pub struct MockRemoteInference;

impl RemoteInference for MockRemoteInference {
    fn infer(&self, request: &InferenceRequest) -> Result<RemoteOutput, InferenceError> {
        // The enclosure has the full fabric, so it can ground even when the local
        // cache could not; it cites the request's grounding ids (or a synthetic one).
        let mut citations = request.local_context_ids.clone();
        if citations.is_empty() {
            citations.push("obj:enclosure-grounding".into());
        }
        Ok(RemoteOutput {
            answer: deterministic_answer(&request.user_message),
            citations,
            grounded: true,
        })
    }
}
