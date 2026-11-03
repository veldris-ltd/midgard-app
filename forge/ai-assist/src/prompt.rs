//! A.6 prompt assembly: render the input schema and the transform library into the
//! injected variables, and pair them with the verbatim A.6 system prompt from the
//! Mimir prompt library. The adapter sends [`AssembledPrompt::full_text`] to the
//! Mimir inference seam. [FR-F-7]
//!
//! Column names come from imported datasets and the request is the user's text, so
//! both reach the model only as provenance-tagged DATA blocks (sources
//! `dataset-schema` and `user`, [`mimir_prompts::delimit`]) under a fresh
//! per-request boundary. The transform library is the product's own vocabulary and
//! stays with the system prompt and the data-handling rule. [VLD-ANB-MIDGARD-0080]

use forge_dataset_store::Schema;
use forge_engine::TransformLibrary;
use mimir_prompts::delimit::{self, Boundary};
use mimir_prompts::{A6_FORGE_NL_TO_TRANSFORM, DATA_HANDLING_RULE};
use pqc_agility::{OsRng, Rng};

/// Labels and sources below are constants and a [`Boundary`] is always a token.
const VALID: &str = "constant label and source, and a Boundary is a token";

/// The A.6 prompt with its three injected sections rendered.
#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    /// The verbatim A.6 system prompt.
    pub system: String,
    /// Rendered `{{SCHEMA}}` — the input dataset's columns and types.
    pub schema_text: String,
    /// Rendered `{{TRANSFORM_LIBRARY}}` — the available op vocabulary.
    pub library_text: String,
    /// The `{{USER_REQUEST}}` — the plain-English ask.
    pub user_request: String,
    /// The boundary this prompt's DATA blocks carry, fresh per assembly.
    pub boundary: Boundary,
}

impl AssembledPrompt {
    /// The system-authority text: the verbatim A.6 prompt, the (trusted) transform
    /// library and the data-handling rule.
    pub fn system_text(&self) -> String {
        format!(
            "{}\n\nTRANSFORM_LIBRARY:\n{}\n\n{DATA_HANDLING_RULE}",
            self.system, self.library_text
        )
    }

    /// The untrusted body: the schema (one bounded item per column line) and the
    /// user request, each as a DATA block.
    pub fn body_text(&self) -> String {
        let b = self.boundary.as_str();
        let columns: Vec<&str> = self.schema_text.lines().collect();
        format!(
            "SCHEMA:\n{}\n\nUSER_REQUEST:\n{}",
            delimit::render_items("SCHEMA", "dataset-schema", b, &columns).expect(VALID),
            delimit::render_block("USER_REQUEST", "user", b, &self.user_request).expect(VALID),
        )
    }

    /// The full text a suggester sends to the model: the system text followed by
    /// the delimited schema and request.
    pub fn full_text(&self) -> String {
        format!("{}\n\n{}\n", self.system_text(), self.body_text())
    }
}

/// Render a schema as `name: type[?]` lines (`?` marks nullable). Column names go
/// through [`delimit::inline`], so each column stays on its one line.
/// [VLD-ANB-MIDGARD-0080]
pub fn render_schema(schema: &Schema) -> String {
    schema
        .columns
        .iter()
        .map(|c| {
            format!(
                "- {}: {}{}",
                delimit::inline(&c.name),
                c.ty.as_str(),
                if c.nullable { "?" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render the transform library op vocabulary.
pub fn render_library() -> String {
    TransformLibrary::op_names()
        .iter()
        .map(|op| format!("- {op}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Assemble the A.6 prompt for a schema and a user request, under a fresh random
/// boundary from `pqc_agility::OsRng`.
pub fn assemble(schema: &Schema, user_request: &str) -> AssembledPrompt {
    let mut bytes = [0u8; 8];
    OsRng.fill(&mut bytes);
    assemble_with_boundary(schema, user_request, Boundary::from_random_bytes(bytes))
}

/// [`assemble`] under a caller-chosen boundary (tests asserting exact layout).
pub fn assemble_with_boundary(
    schema: &Schema,
    user_request: &str,
    boundary: Boundary,
) -> AssembledPrompt {
    AssembledPrompt {
        system: A6_FORGE_NL_TO_TRANSFORM.to_string(),
        schema_text: render_schema(schema),
        library_text: render_library(),
        user_request: user_request.to_string(),
        boundary,
    }
}
