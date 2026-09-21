//! The inference request the adapter serves, tier-independent so the same request
//! can be answered on-device or remotely (tier parity).

/// A single inference request from a client shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceRequest {
    /// The user's message (the lowest-authority layer of the assembled prompt).
    pub user_message: String,
    /// Ids present in the local encrypted cache, usable as grounding.
    pub local_context_ids: Vec<String>,
    /// Whether the task needs data not present in the local cache.
    pub needs_remote_data: bool,
    /// Whether the request would touch data above the session clearance.
    pub above_clearance: bool,
    /// A short POLICY summary injected into the A.C1 prompt (clearance/purpose).
    pub policy_summary: String,
}

impl InferenceRequest {
    /// A minimal request grounded in the given local ids.
    pub fn new(user_message: impl Into<String>, local_context_ids: Vec<String>) -> Self {
        Self {
            user_message: user_message.into(),
            local_context_ids,
            needs_remote_data: false,
            above_clearance: false,
            policy_summary: String::new(),
        }
    }
}
