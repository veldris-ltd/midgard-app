//! A.7 prompt assembly: pair the verbatim A.7 system prompt with the rendered
//! `RELEASE_CONTEXT`. The adapter sends [`AssembledPrompt::full_text`] to the Mimir
//! inference seam. [FR-B-6]
//!
//! The change summary and ring strategy are text a release author wrote, so the
//! release context reaches the model only as a provenance-tagged DATA block (source
//! `release`, [`mimir_prompts::delimit`]) under a fresh per-request boundary, after
//! the verbatim system prompt and the data-handling rule. [VLD-ANB-MIDGARD-0080]

use mimir_prompts::delimit::{self, Boundary};
use mimir_prompts::{A7_BIFROST_RELEASE_RISK, DATA_HANDLING_RULE};
use pqc_agility::{OsRng, Rng};

use crate::context::ReleaseContext;

/// The A.7 prompt with its release-context section rendered.
#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    /// The verbatim A.7 system prompt.
    pub system: String,
    /// The rendered `{{RELEASE_CONTEXT}}`.
    pub release_context: String,
    /// The boundary this prompt's DATA blocks carry, fresh per assembly.
    pub boundary: Boundary,
}

impl AssembledPrompt {
    /// The system-authority text: the verbatim A.7 prompt and the data-handling
    /// rule.
    pub fn system_text(&self) -> String {
        format!("{}\n\n{DATA_HANDLING_RULE}", self.system)
    }

    /// The untrusted body: the release context (one bounded item per fact line)
    /// as a DATA block.
    pub fn body_text(&self) -> String {
        let facts: Vec<&str> = self.release_context.lines().collect();
        format!(
            "RELEASE_CONTEXT:\n{}",
            // A constant label and source, and a Boundary is always a token.
            delimit::render_items("RELEASE_CONTEXT", "release", self.boundary.as_str(), &facts)
                .expect("constant label and source, and a Boundary is a token"),
        )
    }

    /// The full text a risk advisor sends to the model.
    pub fn full_text(&self) -> String {
        format!("{}\n\n{}\n", self.system_text(), self.body_text())
    }
}

/// Assemble the A.7 prompt for a release context, under a fresh random boundary
/// from `pqc_agility::OsRng`.
pub fn assemble(context: &ReleaseContext) -> AssembledPrompt {
    let mut bytes = [0u8; 8];
    OsRng.fill(&mut bytes);
    assemble_with_boundary(context, Boundary::from_random_bytes(bytes))
}

/// [`assemble`] under a caller-chosen boundary (tests asserting exact layout).
pub fn assemble_with_boundary(context: &ReleaseContext, boundary: Boundary) -> AssembledPrompt {
    AssembledPrompt {
        system: A7_BIFROST_RELEASE_RISK.to_string(),
        release_context: context.render(),
        boundary,
    }
}
