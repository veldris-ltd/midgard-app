//! A.8 prompt assembly: render the operating picture into the injected `{{PICTURE}}`
//! (grounded, cite-able tokens) and pair it with the decision point and the verbatim
//! A.8 system prompt from the Mimir prompt library. [FR-H-4]

use mimir_prompts::A8_HEIMDALL_OPTION_GENERATION;

use heimdall_picture::{Attr, Picture};

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
                 track(first={first}, last={last}, n={n}) sources=[{feeds}]{facts}",
                id = e.id.0,
                kind = e.kind,
                layer = e.layer.0,
                first = e.track.first_seen,
                last = e.track.last_seen,
                n = e.track.observation_count,
                facts = facts(&e.latest_attrs),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What an entity reports, briefly, so options can rest on it: up to
/// [`MAX_FACTS`] short attributes (name first), long values (geometry, encoded lists)
/// left out. Empty when it reports none.
pub(crate) fn facts(attrs: &std::collections::BTreeMap<String, Attr>) -> String {
    let short = |a: &Attr| -> Option<String> {
        let s = match a {
            Attr::Text(t) => t.clone(),
            Attr::Int(i) => i.to_string(),
            Attr::Float(f) if f.is_finite() => format!("{}", (f * 1000.0).round() / 1000.0),
            Attr::Bool(b) => b.to_string(),
            _ => return None,
        };
        (!s.is_empty() && s.len() <= 80).then_some(s)
    };
    let mut out: Vec<String> = Vec::new();
    if let Some(name) = attrs.get("name").and_then(short) {
        out.push(format!("name={name:?}"));
    }
    for (k, v) in attrs {
        if out.len() >= MAX_FACTS {
            break;
        }
        if k == "name" || k.ends_with("_at") || k.starts_with("pos_") {
            continue;
        }
        if let Some(s) = short(v) {
            out.push(format!("{k}={s}"));
        }
    }
    if out.is_empty() {
        String::new()
    } else {
        format!(" facts({})", out.join(", "))
    }
}

/// The most attributes [`render_picture`] shows per entity.
pub const MAX_FACTS: usize = 8;

/// Assemble the A.8 prompt for a picture and a decision point.
pub fn assemble(picture: &Picture, decision_point: &str) -> AssembledPrompt {
    AssembledPrompt {
        system: A8_HEIMDALL_OPTION_GENERATION.to_string(),
        picture_text: render_picture(picture),
        decision_point: decision_point.to_string(),
    }
}

#[cfg(test)]
mod facts_tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn facts_lead_with_the_name_and_leave_out_long_values() {
        let mut a = BTreeMap::new();
        a.insert("name".to_string(), Attr::Text("Power cut".into()));
        a.insert("customers".to_string(), Attr::Int(723));
        a.insert("line".to_string(), Attr::Text("5".repeat(200)));
        a.insert("pos_at".to_string(), Attr::Int(1));
        let f = facts(&a);
        assert!(f.starts_with(" facts(name=\"Power cut\""), "{f}");
        assert!(
            f.contains("customers=723") && !f.contains("line=") && !f.contains("pos_at"),
            "{f}"
        );
        assert_eq!(facts(&BTreeMap::new()), "");
    }
}
