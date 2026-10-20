//! Questions over the operating picture, answered by the Mimir core agent (A.1).
//!
//! The caller chooses the evidence: the entities relevant to the question, plus an
//! area summary (counts by kind and the open alerts). They are rendered as the A.1
//! GROUNDING, each citable as `[obj:<id>]` (the summary as [`AREA_SUMMARY_ID`]); the
//! prompt is A.1 verbatim with its default output contract, no tools, and the user's
//! question. Every citation in the answer is resolved against that evidence, and a
//! citation to anything else is reported unresolved, so the console can show what the
//! answer actually rests on. Answers never act: A.1's proposed actions are returned
//! for display only. [FR-MI-3] [FR-H-4] [NFR-AI-1]

use std::collections::BTreeSet;

use heimdall_picture::TrackedEntity;
use mimir_prompts::{A1_DEFAULT_OUTPUT_CONTRACT, A1_MIMIR_CORE_AGENT};

use crate::error::DecisionError;
use crate::json::{self, JsonValue};
use crate::prompt::facts;
use crate::provider::OptionProvider;

/// The citation id of the area summary in the grounding.
pub const AREA_SUMMARY_ID: &str = "AREA-SUMMARY";

/// Mimir's answer to a question over the picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub answer: Option<String>,
    /// Citation ids (`obj:` stripped) as the model gave them.
    pub citations: Vec<String>,
    /// Citations that name nothing in the grounding.
    pub unresolved: Vec<String>,
    /// Non-grounded reasoning the model labelled as such.
    pub inferences: Vec<String>,
    /// A.1 refusal code (UNGROUNDED, POLICY_DENIED, MISSING_TOOL) and detail.
    pub refusal: Option<(String, String)>,
    /// Actions the model proposed: shown only, never executed.
    pub proposed: Vec<String>,
}

impl Answer {
    /// Whether the answer cites something and every citation resolves.
    pub fn grounded(&self) -> bool {
        !self.citations.is_empty() && self.unresolved.is_empty()
    }
}

/// Render the grounding: the area summary, then one citable line per entity.
pub fn render_grounding(entities: &[&TrackedEntity], summary: &str) -> String {
    let mut lines = vec![format!("- [obj:{AREA_SUMMARY_ID}] area summary: {summary}")];
    for e in entities {
        let class = e
            .classification
            .as_ref()
            .map(|c| c.label.as_str())
            .unwrap_or("unclassified");
        let feeds = e.sources.iter().cloned().collect::<Vec<_>>().join(",");
        lines.push(format!(
            "- [obj:{id}] kind={kind} layer={layer} class={class} reports={n} feeds={feeds}{facts}",
            id = e.id.0,
            kind = e.kind,
            layer = e.layer.0,
            n = e.track.observation_count,
            facts = facts(&e.latest_attrs),
        ));
    }
    lines.join("\n")
}

/// The A.1 prompt: the core agent verbatim, then its injected sections.
pub fn assemble_question(grounding: &str, question: &str, purpose: &str) -> String {
    format!(
        "{A1_MIMIR_CORE_AGENT}\n\nPOLICY:\npurpose={purpose}; clearance=UNCLASSIFIED; markings=none\n\n\
         GROUNDING:\n{grounding}\n\nTOOLS:\n(none)\n\nMEMORY:\n(none)\n\nHISTORY:\n(none)\n\n\
         OUTPUT_CONTRACT:\n{A1_DEFAULT_OUTPUT_CONTRACT}\n\nUSER_MESSAGE:\n{question}\n"
    )
}

/// Parse an A.1 response and resolve its citations against `known` ids. Fails closed
/// on anything that is not the A.1 shape.
pub fn parse_answer(text: &str, known: &BTreeSet<String>) -> Result<Answer, DecisionError> {
    let root = json::parse(text)
        .or_else(|e| match (text.find('{'), text.rfind('}')) {
            (Some(a), Some(b)) if a < b => json::parse(&text[a..=b]),
            _ => Err(e),
        })
        .map_err(DecisionError::Parse)?;
    if !matches!(root, JsonValue::Obj(_)) {
        return Err(DecisionError::Parse(
            "the answer is not a JSON object".into(),
        ));
    }
    let strs = |v: Option<&JsonValue>| -> Vec<String> {
        v.and_then(|v| v.as_arr())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let citations: Vec<String> = strs(root.get("citations"))
        .into_iter()
        .map(|c| {
            c.trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim_start_matches("obj:")
                .to_string()
        })
        .filter(|c| !c.is_empty())
        .collect();
    let unresolved = citations
        .iter()
        .filter(|c| !known.contains(*c))
        .cloned()
        .collect();
    let refusal = root.get("refusal").and_then(|r| {
        let code = r
            .get("code")
            .and_then(|c| c.as_str())
            .filter(|c| !c.is_empty() && *c != "null")?;
        Some((
            code.to_string(),
            r.get("detail")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string(),
        ))
    });
    let proposed = root
        .get("proposed_actions")
        .and_then(|v| v.as_arr())
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let t = p.get("action_type").and_then(|x| x.as_str())?;
                    Some(match p.get("rationale").and_then(|x| x.as_str()) {
                        Some(r) if !r.is_empty() => format!("{t}: {r}"),
                        _ => t.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let answer = root
        .get("answer")
        .and_then(|a| a.as_str())
        .map(str::to_string)
        .filter(|a| !a.trim().is_empty());
    // A refusal only stands without an answer: small models fill the field anyway.
    let refusal = if answer.is_some() { None } else { refusal };
    Ok(Answer {
        answer,
        citations,
        unresolved,
        inferences: strs(root.get("inferences")),
        refusal,
        proposed,
    })
}

/// Ask `question` over `entities` (and the area `summary`) through `provider`.
pub fn answer_question<P: OptionProvider + ?Sized>(
    entities: &[&TrackedEntity],
    summary: &str,
    question: &str,
    purpose: &str,
    provider: &P,
) -> Result<Answer, DecisionError> {
    let prompt = assemble_question(&render_grounding(entities, summary), question, purpose);
    let raw = provider.complete(&prompt)?;
    let mut known: BTreeSet<String> = entities.iter().map(|e| e.id.0.clone()).collect();
    known.insert(AREA_SUMMARY_ID.to_string());
    parse_answer(&raw, &known)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockOptionProvider;

    #[test]
    fn an_answer_is_grounded_only_when_every_citation_is_known() {
        let known: BTreeSet<String> = ["PWR-1".to_string(), AREA_SUMMARY_ID.to_string()].into();
        let a = parse_answer(
            r#"{"plan":["look"],"answer":"One cut.","citations":["obj:PWR-1","[obj:AREA-SUMMARY]"],
            "proposed_actions":[],"inferences":[],"refusal":{"code":null,"detail":null}}"#,
            &known,
        )
        .unwrap();
        assert!(a.grounded());
        assert_eq!(a.answer.as_deref(), Some("One cut."));
        assert!(a.refusal.is_none());
        let b = parse_answer(
            r#"{"answer":"Two.","citations":["obj:PWR-9"],"refusal":{"code":"UNGROUNDED","detail":"none needed"}}"#,
            &known,
        )
        .unwrap();
        assert_eq!(b.unresolved, vec!["PWR-9".to_string()]);
        assert!(b.refusal.is_none(), "an answer overrides a stray refusal");
        assert!(!b.grounded());
    }

    #[test]
    fn a_refusal_and_proposals_are_read_and_prose_fails_closed() {
        let known = BTreeSet::new();
        let a = parse_answer(r#"{"answer":null,"citations":[],"proposed_actions":[{"action_type":"notify","rationale":"tell ops"}],
            "refusal":{"code":"UNGROUNDED","detail":"no evidence"}}"#, &known).unwrap();
        assert_eq!(a.refusal, Some(("UNGROUNDED".into(), "no evidence".into())));
        assert_eq!(a.proposed, vec!["notify: tell ops".to_string()]);
        assert!(parse_answer("It is fine.", &known).is_err());
    }

    #[test]
    fn the_prompt_is_a1_with_its_sections() {
        let p = assemble_question(
            "- [obj:AREA-SUMMARY] area summary: none",
            "Any cuts?",
            "investigation",
        );
        assert!(p.starts_with(A1_MIMIR_CORE_AGENT));
        for s in [
            "POLICY:",
            "GROUNDING:",
            "TOOLS:",
            "OUTPUT_CONTRACT:",
            "USER_MESSAGE:\nAny cuts?",
        ] {
            assert!(p.contains(s), "{s}");
        }
        let mock =
            MockOptionProvider::new(r#"{"answer":"None.","citations":["obj:AREA-SUMMARY"]}"#);
        let a = answer_question(&[], "nothing", "Any cuts?", "investigation", &mock).unwrap();
        assert!(a.grounded());
    }
}
