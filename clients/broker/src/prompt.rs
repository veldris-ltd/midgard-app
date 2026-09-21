//! Assembly of the verbatim A.C3 extension-assistant prompt. The authoritative text
//! lives in [`mimir_prompts`]; this only substitutes the runtime-injected variables
//! (`PAGE_SELECTION`, `GROUNDING`, `POLICY`). `[KDD-1]`

use mimir_prompts::{A_C3_DEFAULT_OUTPUT_CONTRACT, A_C3_EXTENSION_ASSISTANT};

/// Assemble the A.C3 prompt from the user-shared selection, enclosure grounding, and
/// the policy summary. Note the broker only ever passes the *shared* selection here.
pub fn assemble(shared_selection: &str, grounding: &[String], policy_summary: &str) -> String {
    let grounding_block = if grounding.is_empty() {
        "(no grounding)".to_string()
    } else {
        grounding.join(", ")
    };
    format!(
        "{system}\n\nPOLICY:\n{policy}\n\nPAGE_SELECTION:\n{selection}\n\nGROUNDING:\n{grounding}\n\n\
         OUTPUT_CONTRACT:\n{contract}",
        system = A_C3_EXTENSION_ASSISTANT,
        policy = if policy_summary.is_empty() {
            "(session policy)"
        } else {
            policy_summary
        },
        selection = shared_selection,
        grounding = grounding_block,
        contract = A_C3_DEFAULT_OUTPUT_CONTRACT,
    )
}
