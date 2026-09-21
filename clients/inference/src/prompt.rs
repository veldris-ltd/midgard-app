//! Assembly of the verbatim A.C1 on-device assistant prompt. The authoritative
//! text lives in [`mimir_prompts`]; this only substitutes the runtime-injected
//! `{{DOUBLE_BRACE}}` variables, in the authority order of BUILD_PLAYBOOK §0.1
//! (system → policy → grounding → … → user message). `[KDD-1]`

use mimir_prompts::{A_C1_DEFAULT_OUTPUT_CONTRACT, A_C1_ON_DEVICE_ASSISTANT, A_C2_TIER_SELECTION};

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

/// Assemble the A.C1 prompt for an on-device (or hybrid draft) turn. Higher-authority
/// layers come first so lower-authority content cannot override them.
pub fn assemble_on_device(request: &InferenceRequest, tier: Tier) -> String {
    let local_context = if request.local_context_ids.is_empty() {
        "(no local objects)".to_string()
    } else {
        request.local_context_ids.join(", ")
    };
    format!(
        "{system}\n\nPOLICY:\n{policy}\n\nLOCAL_CONTEXT:\n{context}\n\nTIER: {tier}\n\n\
         OUTPUT_CONTRACT:\n{contract}\n\nUSER_MESSAGE:\n{message}",
        system = A_C1_ON_DEVICE_ASSISTANT,
        policy = if request.policy_summary.is_empty() {
            "(default scope)"
        } else {
            &request.policy_summary
        },
        context = local_context,
        tier = tier_label(tier),
        contract = A_C1_DEFAULT_OUTPUT_CONTRACT,
        message = request.user_message,
    )
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
