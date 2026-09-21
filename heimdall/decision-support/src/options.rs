//! The parsed A.8 option set: ranked courses of action, each citing specific
//! evidence. **Advisory** — this type carries no way to act; the workflow gate and
//! a human decide. [FR-H-4] [NFR-AI-1]

use crate::error::DecisionError;
use crate::json::{self, JsonValue};

/// What kind of picture element a citation points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CitationKind {
    /// A tracked entity (`entity:<id>`).
    Entity,
    /// A feed source (`feed:<id>`).
    Feed,
    /// A track (`track:<id>`) — in this phase a track shares its entity's id.
    Track,
    /// An unrecognised citation prefix — never grounds (fails closed).
    Unknown,
}

/// A single piece of cited evidence backing an option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    /// The kind of element cited.
    pub kind: CitationKind,
    /// The referenced id (the part after the `kind:` prefix).
    pub id: String,
    /// The original token as emitted by the model (for audit/display).
    pub raw: String,
}

impl Citation {
    /// Parse a `kind:id` citation token. An unrecognised prefix (or a token with no
    /// prefix) becomes [`CitationKind::Unknown`], which never grounds.
    pub fn parse(token: &str) -> Self {
        let raw = token.to_string();
        match token.split_once(':') {
            Some(("entity", id)) => Citation {
                kind: CitationKind::Entity,
                id: id.to_string(),
                raw,
            },
            Some(("feed", id)) => Citation {
                kind: CitationKind::Feed,
                id: id.to_string(),
                raw,
            },
            Some(("track", id)) => Citation {
                kind: CitationKind::Track,
                id: id.to_string(),
                raw,
            },
            _ => Citation {
                kind: CitationKind::Unknown,
                id: token.to_string(),
                raw,
            },
        }
    }
}

/// A ranked course of action proposed by A.8. Cited, advisory, never executed by
/// this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseOfAction {
    /// Rank (1 = best); distinct across an option set.
    pub rank: u32,
    /// Short title.
    pub title: String,
    /// Plain-English summary.
    pub summary: String,
    /// The specific evidence this option cites. [FR-H-4]
    pub citations: Vec<Citation>,
    /// The model's stated expected effect.
    pub expected_effect: String,
    /// The model's stated risks.
    pub risks: Vec<String>,
}

/// A parsed set of advisory options. If the model could not support a decision from
/// the picture, `options` is empty and `unsupported` explains why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionSet {
    /// The ranked options.
    pub options: Vec<CourseOfAction>,
    /// Set iff the picture could not support a decision.
    pub unsupported: Option<String>,
}

impl OptionSet {
    /// Parse an A.8 JSON response. Fails closed on malformed structure.
    pub fn parse(text: &str) -> Result<Self, DecisionError> {
        let root = json::parse(text).map_err(DecisionError::Parse)?;

        let unsupported = match root.get("unsupported") {
            None => None,
            Some(v) if v.is_null() => None,
            Some(v) => v
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string()),
        };

        let options = match root.get("options").and_then(|v| v.as_arr()) {
            None => Vec::new(),
            Some(items) => items.iter().map(parse_option).collect::<Result<_, _>>()?,
        };

        Ok(OptionSet {
            options,
            unsupported,
        })
    }

    /// Whether the ranks are well-formed: strictly increasing and distinct, starting
    /// at 1 (`1, 2, 3, …`). An empty set is trivially well-ranked.
    pub fn is_well_ranked(&self) -> bool {
        self.options
            .iter()
            .enumerate()
            .all(|(i, o)| o.rank as usize == i + 1)
    }
}

fn parse_option(v: &JsonValue) -> Result<CourseOfAction, DecisionError> {
    let rank = v
        .get("rank")
        .and_then(|x| x.as_i64())
        .filter(|n| *n >= 1)
        .ok_or_else(|| DecisionError::Parse("option.rank must be an integer >= 1".into()))?
        as u32;
    let title = str_field(v, "title");
    let summary = str_field(v, "summary");
    let expected_effect = str_field(v, "expected_effect");
    let citations = match v.get("citations").and_then(|x| x.as_arr()) {
        None => Vec::new(),
        Some(items) => items
            .iter()
            .filter_map(|x| x.as_str())
            .map(Citation::parse)
            .collect(),
    };
    let risks = match v.get("risks").and_then(|x| x.as_arr()) {
        None => Vec::new(),
        Some(items) => items
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect(),
    };
    Ok(CourseOfAction {
        rank,
        title,
        summary,
        citations,
        expected_effect,
        risks,
    })
}

fn str_field(v: &JsonValue, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}
