//! Assembly of the verbatim A.C1 on-device assistant prompt. The authoritative
//! text lives in [`mimir_prompts`]; this only substitutes the runtime-injected
//! `{{DOUBLE_BRACE}}` variables, in the authority order of BUILD_PLAYBOOK §0.1
//! (system → policy → grounding → … → user message). `[KDD-1]`
//!
//! The local context ids come from the device store and the user message is the
//! user's text, so in A.C1 both reach the model only as provenance-tagged DATA
//! blocks (sources `local-store` and `user`, [`mimir_prompts::delimit`]) under a
//! fresh per-request boundary; the policy, tier and output contract stay with the
//! system prompt and the data-handling rule. The A.C2 prompt holds only booleans,
//! numbers and enums the client computes, so it has no untrusted text to delimit.
//! The crate has a CSPRNG only under `backend-fips` (tests run on the test
//! backend), so the boundary comes from [`Boundary::from_std_entropy`].
//! [VLD-ANB-MIDGARD-0080] [FR-C-6]

use mimir_prompts::delimit::{self, Boundary};
use mimir_prompts::{
    A_C1_DEFAULT_OUTPUT_CONTRACT, A_C1_ON_DEVICE_ASSISTANT, A_C2_TIER_SELECTION, DATA_HANDLING_RULE,
};

use crate::request::InferenceRequest;
use crate::tier::{Tier, TierContext};

/// The tier string A.C1 expects in its `{{TIER}}` slot.
fn tier_label(tier: Tier) -> &'static str {
    match tier {
        Tier::OnDevice => "on_device",
        Tier::Remote => "remote",
        Tier::Hybrid => "hybrid",
        Tier::Disable => "disable",
    }
}

/// Labels and sources below are constants and a [`Boundary`] is always a token.
const VALID: &str = "constant label and source, and a Boundary is a token";

/// The A.C1 prompt split into its system-authority text and its delimited body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledOnDevicePrompt {
    /// The verbatim A.C1 prompt, the policy summary, the tier, the output contract
    /// and the data-handling rule.
    pub system_text: String,
    /// The local context and the user message, each as a DATA block.
    pub body_text: String,
    /// The boundary this prompt's DATA blocks carry, fresh per assembly.
    pub boundary: Boundary,
}

impl AssembledOnDevicePrompt {
    /// The full text sent to the model.
    pub fn full_text(&self) -> String {
        format!("{}\n\n{}", self.system_text, self.body_text)
    }
}

/// Assemble the A.C1 prompt for an on-device (or hybrid draft) turn. Higher-authority
/// layers come first so lower-authority content cannot override them.
pub fn assemble_on_device(request: &InferenceRequest, tier: Tier) -> String {
    assemble_on_device_with_boundary(request, tier, Boundary::from_std_entropy()).full_text()
}

/// [`assemble_on_device`] split into its system text and delimited body, under a
/// caller-chosen boundary.
pub fn assemble_on_device_with_boundary(
    request: &InferenceRequest,
    tier: Tier,
    boundary: Boundary,
) -> AssembledOnDevicePrompt {
    let system_text = format!(
        "{system}\n\nPOLICY:\n{policy}\n\nTIER: {tier}\n\n\
         OUTPUT_CONTRACT:\n{contract}\n\n{DATA_HANDLING_RULE}",
        system = A_C1_ON_DEVICE_ASSISTANT,
        policy = if request.policy_summary.is_empty() {
            "(default scope)"
        } else {
            &request.policy_summary
        },
        tier = tier_label(tier),
        contract = A_C1_DEFAULT_OUTPUT_CONTRACT,
    );
    let b = boundary.as_str();
    let local_context = if request.local_context_ids.is_empty() {
        "(no local objects)".to_string()
    } else {
        let ids: Vec<String> = request
            .local_context_ids
            .iter()
            .map(|id| delimit::inline(id))
            .collect();
        delimit::render_items("LOCAL_CONTEXT", "local-store", b, &ids).expect(VALID)
    };
    let body_text = format!(
        "LOCAL_CONTEXT:\n{local_context}\n\nUSER_MESSAGE:\n{message}",
        message =
            delimit::render_block("USER_MESSAGE", "user", b, &request.user_message).expect(VALID),
    );
    AssembledOnDevicePrompt {
        system_text,
        body_text,
        boundary,
    }
}

/// Assemble the A.C2 tier-selection prompt for the ambiguous case. Production sends
/// this to Mimir; the deterministic guard still has the final say.
pub fn assemble_tier_selection(ctx: &TierContext) -> String {
    format!(
        "{system}\n\nDEVICE_CAPABILITY:\n ram_mb={ram}, accelerator={acc}, model_loaded={model}\n\n\
         CONNECTIVITY: {conn:?}\n\nPOLICY:\n allow_egress={egress}, privacy={privacy:?}\n\n\
         TASK_COMPLEXITY: {complexity:?}",
        system = A_C2_TIER_SELECTION,
        ram = ctx.capability.ram_mb,
        acc = ctx.capability.has_accelerator,
        model = ctx.capability.on_device_model,
        conn = ctx.connectivity,
        egress = ctx.allow_egress,
        privacy = ctx.privacy,
        complexity = ctx.complexity,
    )
}
