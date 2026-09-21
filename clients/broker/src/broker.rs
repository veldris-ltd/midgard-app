//! The broker itself: it turns a user task over a shared page selection into an
//! assistant answer plus brokered requests, reading nothing it was not given and
//! emitting no secret. `[FR-C-8]` `[AC-C-8]`

use crate::assistant::InPageAssistant;
use crate::error::BrokerError;

/// The page context handed to the broker. The broker reads **only**
/// [`shared_selection`](PageContext::shared_selection). The other fields model
/// what the extension *could* see but must not — they exist so tests can prove the
/// broker never reads them (EV-AC3-noscrape).
#[derive(Debug, Clone, Default)]
pub struct PageContext {
    /// The text the user explicitly shared. The only readable input.
    pub shared_selection: String,
    /// Content of other tabs — MUST NOT be read.
    pub other_tabs: Vec<String>,
    /// Credentials / form fields on the page — MUST NOT be read.
    pub form_secrets: Vec<String>,
}

impl PageContext {
    /// A context with only a shared selection (the common case).
    pub fn shared(selection: impl Into<String>) -> Self {
        Self {
            shared_selection: selection.into(),
            other_tabs: Vec::new(),
            form_secrets: Vec::new(),
        }
    }
}

/// A privileged operation the in-page task might need — always brokered, never
/// performed in-page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegedOp {
    /// The operation name (e.g. `seal`, `sign`, `fetch_marked_object`).
    pub op: String,
    /// Non-secret parameters for the operation.
    pub params: Vec<(String, String)>,
}

/// A request forwarded to the enclosure / companion app over the authenticated
/// channel. The broker emits these instead of doing privileged work itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokeredRequest {
    /// The privileged operation to perform.
    pub op: PrivilegedOp,
    /// The channel it is brokered to (always the enclosure).
    pub channel: String,
}

/// A structured refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// `OUT_OF_SCOPE` or `POLICY_DENIED`.
    pub code: String,
    /// Detail.
    pub detail: Option<String>,
}

/// The A.C3 output contract, typed. Privileged work appears only as
/// [`brokered_requests`](ExtensionOutput::brokered_requests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionOutput {
    /// The answer, or `None` if refused.
    pub answer: Option<String>,
    /// Enclosure grounding citations (`obj:<id>`).
    pub citations: Vec<String>,
    /// Privileged operations to broker to the enclosure (never done in-page).
    pub brokered_requests: Vec<BrokeredRequest>,
    /// A refusal, if the task is out of extension scope.
    pub refusal: Option<Refusal>,
}

/// A user task for the extension assistant.
#[derive(Debug, Clone)]
pub struct Task {
    /// The user's request.
    pub message: String,
    /// Whether the task requires a privileged operation (seal/sign/marked fetch).
    /// When set, the broker emits a brokered request rather than attempting it.
    pub privileged_op: Option<PrivilegedOp>,
}

impl Task {
    /// A plain (non-privileged) task.
    pub fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            privileged_op: None,
        }
    }
    /// A task that needs a privileged operation (which will be brokered).
    pub fn privileged(message: impl Into<String>, op: PrivilegedOp) -> Self {
        Self {
            message: message.into(),
            privileged_op: Some(op),
        }
    }
}

/// Run the extension assistant over a shared page selection. Reads only
/// `page.shared_selection` and `grounding`; brokers any privileged op; never emits
/// a secret. `[FR-C-8]` `[AC-C-8]`
pub fn assist<A: InPageAssistant>(
    page: &PageContext,
    grounding: &[String],
    task: &Task,
    assistant: &A,
) -> Result<ExtensionOutput, BrokerError> {
    // A privileged task is brokered, not attempted in-page. The broker does not — and
    // structurally cannot — perform the operation; it only forwards it.
    if let Some(op) = &task.privileged_op {
        return Ok(ExtensionOutput {
            answer: None,
            citations: grounding.to_vec(),
            brokered_requests: vec![BrokeredRequest {
                op: op.clone(),
                channel: "enclosure".to_string(),
            }],
            refusal: None,
        });
    }

    // A plain task: the assistant reasons over ONLY the shared selection + grounding.
    // The other-tab / form-secret fields of `page` are never passed in.
    let answer = assistant
        .assist(&page.shared_selection, grounding, &task.message)
        .map_err(|e| BrokerError::Assistant(e.to_string()))?;

    Ok(ExtensionOutput {
        answer: Some(answer),
        citations: grounding.to_vec(),
        brokered_requests: Vec::new(),
        refusal: None,
    })
}
