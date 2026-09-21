//! A.8 prompt assembly: render the operating picture into the injected `{{PICTURE}}`
//! (grounded, cite-able tokens) and pair it with the decision point and the verbatim
//! A.8 system prompt from the Mimir prompt library. [FR-H-4]

use mimir_prompts::A8_HEIMDALL_OPTION_GENERATION;

use heimdall_picture::Picture;

/// The A.8 prompt with its two injected sections rendered.
#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    /// The verbatim A.8 system prompt.
    pub system: String,
    /// Rendered `{{PICTURE}}` — the cite-able entities, classifications, and feeds.
    pub picture_text: String,
    /// The `{{DECISION_POINT}}` — the operator's question.
    pub decision_point: String,
}

impl AssembledPrompt {
    /// The full text a provider sends to the model.
    pub fn full_text(&self) -> String {
        format!(
            "{}\n\nPICTURE:\n{}\n\nDECISION_POINT:\n{}\n",
            self.system, self.picture_text, self.decision_point
        )
    }
}

/// Render the picture as grounded, cite-able lines. Each entity line exposes the
/// exact tokens A.8 must cite: `entity:<id>`, and `feed:<id>` for each contributing
/// source.
pub fn render_picture(picture: &Picture) -> String {
    if picture.entity_count() == 0 {
        return "(empty picture)".to_string();
    }
    picture
        .entities()
        .map(|e| {
            let class = e
                .classification
                .as_ref()
                .map(|c| c.label.as_str())
                .unwrap_or("unclassified");
            let feeds = e
                .sources
                .iter()
                .map(|f| format!("feed:{f}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "- entity:{id} kind={kind} layer={layer} class={class} \
                 track(first={first}, last={last}, n={n}) sources=[{feeds}]",
                id = e.id.0,
                kind = e.kind,
                layer = e.layer.0,
                first = e.track.first_seen,
                last = e.track.last_seen,
                n = e.track.observation_count,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Assemble the A.8 prompt for a picture and a decision point.
pub fn assemble(picture: &Picture, decision_point: &str) -> AssembledPrompt {
    AssembledPrompt {
        system: A8_HEIMDALL_OPTION_GENERATION.to_string(),
        picture_text: render_picture(picture),
        decision_point: decision_point.to_string(),
    }
}
