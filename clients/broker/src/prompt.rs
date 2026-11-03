//! Assembly of the verbatim A.C3 extension-assistant prompt. The authoritative text
//! lives in [`mimir_prompts`]; this only substitutes the runtime-injected variables
//! (`PAGE_SELECTION`, `GROUNDING`, `POLICY`). `[KDD-1]`
//!
//! The page selection is web content and the grounding comes back from the
//! enclosure, so both reach the model only as provenance-tagged DATA blocks
//! (sources `web-page` and `enclosure`, [`mimir_prompts::delimit`]) under a fresh
//! per-request boundary. The policy summary and the output contract are the
//! product's own and stay with the system prompt and the data-handling rule.
//! [VLD-ANB-MIDGARD-0080] [FR-C-8]

use mimir_prompts::delimit::{self, Boundary};
use mimir_prompts::{A_C3_DEFAULT_OUTPUT_CONTRACT, A_C3_EXTENSION_ASSISTANT, DATA_HANDLING_RULE};

/// Labels and sources below are constants and a [`Boundary`] is always a token.
const VALID: &str = "constant label and source, and a Boundary is a token";

/// The A.C3 prompt split into its system-authority text and its delimited body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledExtensionPrompt {
    /// The verbatim A.C3 prompt, the policy summary, the output contract and the
    /// data-handling rule. Nothing from the page or the grounding is in it.
    pub system_text: String,
    /// The page selection and the grounding, each as a DATA block.
    pub body_text: String,
    /// The boundary this prompt's DATA blocks carry, fresh per assembly.
    pub boundary: Boundary,
}

impl AssembledExtensionPrompt {
    /// The full text sent to the model.
    pub fn full_text(&self) -> String {
        format!("{}\n\n{}", self.system_text, self.body_text)
    }
}

/// A fresh boundary for one prompt. The broker performs no cryptography and takes
/// no crypto dependency (KDD-2), so it uses [`Boundary::from_std_entropy`], not a
/// CSPRNG. [VLD-ANB-MIDGARD-0080]
fn request_boundary() -> Boundary {
    Boundary::from_std_entropy()
}

/// Assemble the A.C3 prompt from the user-shared selection, enclosure grounding, and
/// the policy summary. Note the broker only ever passes the *shared* selection here.
pub fn assemble(shared_selection: &str, grounding: &[String], policy_summary: &str) -> String {
    assemble_with_boundary(
        shared_selection,
        grounding,
        policy_summary,
        request_boundary(),
    )
    .full_text()
}

/// [`assemble`] split into its system text and delimited body, under a
/// caller-chosen boundary.
pub fn assemble_with_boundary(
    shared_selection: &str,
    grounding: &[String],
    policy_summary: &str,
    boundary: Boundary,
) -> AssembledExtensionPrompt {
    let system_text = format!(
        "{system}\n\nPOLICY:\n{policy}\n\nOUTPUT_CONTRACT:\n{contract}\n\n{DATA_HANDLING_RULE}",
        system = A_C3_EXTENSION_ASSISTANT,
        policy = if policy_summary.is_empty() {
            "(session policy)"
        } else {
            policy_summary
        },
        contract = A_C3_DEFAULT_OUTPUT_CONTRACT,
    );
    let b = boundary.as_str();
    let grounding_block = if grounding.is_empty() {
        "(no grounding)".to_string()
    } else {
        delimit::render_items("GROUNDING", "enclosure", b, grounding).expect(VALID)
    };
    let body_text = format!(
        "PAGE_SELECTION:\n{selection}\n\nGROUNDING:\n{grounding}",
        selection =
            delimit::render_block("PAGE_SELECTION", "web-page", b, shared_selection).expect(VALID),
        grounding = grounding_block,
    );
    AssembledExtensionPrompt {
        system_text,
        body_text,
        boundary,
    }
}
