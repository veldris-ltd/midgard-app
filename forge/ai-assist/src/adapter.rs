//! The AI-assist adapter. It assembles the A.6 prompt, calls the Mimir inference
//! seam, parses the response into typed stages, and **type-checks them against the
//! input schema** to produce a [`TransformPreview`] — a proposed, diffable change.
//!
//! The invariant that satisfies AC-F-5 / NFR-AI-1: the adapter has **no code path
//! that commits**. A preview's `committed` flag is private and always `false`; the
//! only way its stages leave the adapter is [`TransformPreview::approved_stages`],
//! which a human calls to build a pipeline themselves. The AI never mutates a
//! dataset. [FR-F-7] [NFR-AI-1] [AC-F-5]

use forge_dataset_store::Schema;
use forge_engine::{ExecutionPlan, Transform};

use crate::error::AssistError;
use crate::prompt::assemble;
use crate::proposal::parse_proposal;
use crate::suggester::TransformSuggester;

/// Whether a proposed transform can actually be applied to the input schema.
#[derive(Debug, Clone)]
pub enum Validity {
    /// The stages type-check; `output_schema` is the resulting shape (the "after"
    /// side of the preview diff).
    Valid {
        /// The input ("before") schema.
        input_schema: Schema,
        /// The inferred output ("after") schema.
        output_schema: Schema,
    },
    /// The stages reference a column/type the schema does not support (e.g. an
    /// invented column) — surfaced, never silently applied.
    Invalid {
        /// The type-checker's reason.
        reason: String,
    },
    /// The model reported it could not satisfy the request with the library.
    Unsupported {
        /// The model's stated reason.
        reason: String,
    },
}

/// A previewable transform proposal. Always a proposal — never committed here.
#[derive(Debug, Clone)]
pub struct TransformPreview {
    /// The proposed transform stages (the engine IR).
    pub stages: Vec<Transform>,
    /// The model's plain-English explanation.
    pub explanation: String,
    /// Whether/how the proposal can be applied.
    pub validity: Validity,
    // Deliberately private with no setter: the adapter never commits, so this is
    // structurally always false. [AC-F-5]
    committed: bool,
}

impl TransformPreview {
    /// Always `false`: the AI path produces previews, never commits. [AC-F-5]
    pub fn is_committed(&self) -> bool {
        self.committed
    }

    /// Whether the proposal type-checks and could be applied on approval.
    pub fn is_valid(&self) -> bool {
        matches!(self.validity, Validity::Valid { .. })
    }

    /// The inferred output schema, when valid (the "after" of the diff).
    pub fn output_schema(&self) -> Option<&Schema> {
        match &self.validity {
            Validity::Valid { output_schema, .. } => Some(output_schema),
            _ => None,
        }
    }

    /// **Human approval step.** Returns the stages for the caller to build and run a
    /// pipeline with — only when the preview is valid. This is the sole way stages
    /// leave the adapter, and applying them is the caller's explicit action; the
    /// adapter itself never runs or commits them. [NFR-AI-1]
    pub fn approved_stages(&self) -> Option<&[Transform]> {
        if self.is_valid() {
            Some(&self.stages)
        } else {
            None
        }
    }
}

/// Ask Mimir (via `suggester`) to turn `user_request` into a previewable transform
/// over `schema`. The result is always a proposal; nothing is committed. Fails
/// closed if inference fails or the response is malformed. [FR-F-7] [AC-F-5]
pub fn suggest_transform<S: TransformSuggester>(
    schema: &Schema,
    user_request: &str,
    suggester: &S,
) -> Result<TransformPreview, AssistError> {
    // Assemble the A.6 prompt and call the model.
    let prompt = assemble(schema, user_request);
    let raw_text = suggester.suggest(&prompt)?;
    let raw = parse_proposal(&raw_text)?;

    let validity = match raw.unsupported {
        Some(reason) if !reason.trim().is_empty() => Validity::Unsupported { reason },
        _ => match ExecutionPlan::new(raw.stages.clone()).output_schema(schema) {
            Ok(output_schema) => Validity::Valid {
                input_schema: schema.clone(),
                output_schema,
            },
            Err(e) => Validity::Invalid {
                reason: e.to_string(),
            },
        },
    };

    Ok(TransformPreview {
        stages: raw.stages,
        explanation: raw.explanation,
        validity,
        committed: false,
    })
}
