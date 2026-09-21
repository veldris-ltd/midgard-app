//! A.7 prompt assembly: pair the verbatim A.7 system prompt with the rendered
//! `RELEASE_CONTEXT`. The adapter sends [`AssembledPrompt::full_text`] to the Mimir
//! inference seam. [FR-B-6]

use mimir_prompts::A7_BIFROST_RELEASE_RISK;

use crate::context::ReleaseContext;

/// The A.7 prompt with its release-context section rendered.
#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    /// The verbatim A.7 system prompt.
    pub system: String,
    /// The rendered `{{RELEASE_CONTEXT}}`.
    pub release_context: String,
}

impl AssembledPrompt {
    /// The full text a risk advisor sends to the model.
    pub fn full_text(&self) -> String {
        format!(
            "{}\n\nRELEASE_CONTEXT:\n{}\n",
            self.system, self.release_context
        )
    }
}

/// Assemble the A.7 prompt for a release context.
pub fn assemble(context: &ReleaseContext) -> AssembledPrompt {
    AssembledPrompt {
        system: A7_BIFROST_RELEASE_RISK.to_string(),
        release_context: context.render(),
    }
}
