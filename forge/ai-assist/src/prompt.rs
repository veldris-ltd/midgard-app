//! A.6 prompt assembly: render the input schema and the transform library into the
//! injected variables, and pair them with the verbatim A.6 system prompt from the
//! Mimir prompt library. The adapter sends [`AssembledPrompt::full_text`] to the
//! Mimir inference seam. [FR-F-7]

use forge_dataset_store::Schema;
use forge_engine::TransformLibrary;
use mimir_prompts::A6_FORGE_NL_TO_TRANSFORM;

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
}

impl AssembledPrompt {
    /// The full text a suggester sends to the model: system prompt followed by the
    /// three injected sections, in the documented order.
    pub fn full_text(&self) -> String {
        format!(
            "{}\n\nSCHEMA:\n{}\n\nTRANSFORM_LIBRARY:\n{}\n\nUSER_REQUEST:\n{}\n",
            self.system, self.schema_text, self.library_text, self.user_request
        )
    }
}

/// Render a schema as `name: type[?]` lines (`?` marks nullable).
pub fn render_schema(schema: &Schema) -> String {
    schema
        .columns
        .iter()
        .map(|c| {
            format!(
                "- {}: {}{}",
                c.name,
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

/// Assemble the A.6 prompt for a schema and a user request.
pub fn assemble(schema: &Schema, user_request: &str) -> AssembledPrompt {
    AssembledPrompt {
        system: A6_FORGE_NL_TO_TRANSFORM.to_string(),
        schema_text: render_schema(schema),
        library_text: render_library(),
        user_request: user_request.to_string(),
    }
}
