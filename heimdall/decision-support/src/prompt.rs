//! A.8 prompt assembly: render the operating picture into the injected `{{PICTURE}}`
//! (grounded, cite-able tokens) and pair it with the decision point and the verbatim
//! A.8 system prompt from the Mimir prompt library. [FR-H-4]
//!
//! The picture carries feed-supplied entity fields and the decision point is the
//! operator's text, so both reach the model only as provenance-tagged DATA blocks
//! (sources `picture` and `operator`, [`mimir_prompts::delimit`]) under a fresh
//! per-request boundary, after the verbatim system prompt and the data-handling
//! rule. A feed cannot forge a header or an instruction. [VLD-ANB-MIDGARD-0080]

use mimir_prompts::delimit::{self, Boundary};
use mimir_prompts::{A8_HEIMDALL_OPTION_GENERATION, DATA_HANDLING_RULE};
use pqc_agility::{OsRng, Rng};

use heimdall_picture::Picture;

/// Labels and sources below are constants and a [`Boundary`] is always a token.
const VALID: &str = "constant label and source, and a Boundary is a token";

/// The A.8 prompt with its two injected sections rendered.
#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    /// The verbatim A.8 system prompt.
    pub system: String,
    /// Rendered `{{PICTURE}}` — the cite-able entities, classifications, and feeds.
    pub picture_text: String,
    /// The `{{DECISION_POINT}}` — the operator's question.
    pub decision_point: String,
    /// The boundary this prompt's DATA blocks carry, fresh per assembly.
    pub boundary: Boundary,
}

impl AssembledPrompt {
    /// The system-authority text: the verbatim A.8 prompt and the data-handling
    /// rule. Nothing from the picture or the operator is in it.
    pub fn system_text(&self) -> String {
        format!("{}\n\n{DATA_HANDLING_RULE}", self.system)
    }

    /// The untrusted body: the picture (one bounded item per entity line) and the
    /// decision point, each as a DATA block.
    pub fn body_text(&self) -> String {
        let b = self.boundary.as_str();
        let entities: Vec<&str> = self.picture_text.lines().collect();
        format!(
            "PICTURE:\n{}\n\nDECISION_POINT:\n{}",
            delimit::render_items("PICTURE", "picture", b, &entities).expect(VALID),
            delimit::render_block("DECISION_POINT", "operator", b, &self.decision_point)
                .expect(VALID),
        )
    }

    /// The full text a provider sends to the model.
    pub fn full_text(&self) -> String {
        format!("{}\n\n{}\n", self.system_text(), self.body_text())
    }
}

/// Render the picture as grounded, cite-able lines. Each entity line exposes the
/// exact tokens A.8 must cite: `entity:<id>`, and `feed:<id>` for each contributing
/// source. Every feed-supplied field goes through [`delimit::inline`], so an entity
/// stays on its one line. [VLD-ANB-MIDGARD-0080]
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
                .map(|f| format!("feed:{}", delimit::inline(f)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "- entity:{id} kind={kind} layer={layer} class={class} \
                 track(first={first}, last={last}, n={n}) sources=[{feeds}]",
                id = delimit::inline(&e.id.0),
                kind = delimit::inline(&e.kind),
                layer = delimit::inline(&e.layer.0),
                class = delimit::inline(class),
                first = e.track.first_seen,
                last = e.track.last_seen,
                n = e.track.observation_count,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Assemble the A.8 prompt for a picture and a decision point, under a fresh
/// random boundary from `pqc_agility::OsRng`.
pub fn assemble(picture: &Picture, decision_point: &str) -> AssembledPrompt {
    let mut bytes = [0u8; 8];
    OsRng.fill(&mut bytes);
    assemble_with_boundary(picture, decision_point, Boundary::from_random_bytes(bytes))
}

/// [`assemble`] under a caller-chosen boundary (tests asserting exact layout).
pub fn assemble_with_boundary(
    picture: &Picture,
    decision_point: &str,
    boundary: Boundary,
) -> AssembledPrompt {
    AssembledPrompt {
        system: A8_HEIMDALL_OPTION_GENERATION.to_string(),
        picture_text: render_picture(picture),
        decision_point: decision_point.to_string(),
        boundary,
    }
}
