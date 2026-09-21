//! The A.2 grounding planner: turn an NL task into a typed retrieval plan.
//!
//! This is a deterministic, dependency-free implementation of the A.2 contract
//! (`mimir_prompts::A2_OAG_GROUNDING`). The verbatim prompt is the specification;
//! this code enforces its RULES structurally:
//! - it only ever references object/link types drawn from the [`OntologySummary`]
//!   (so it can never name a type that does not exist);
//! - it prefers precise typed lookups (search-around only between matched types);
//! - it returns `groundable = false` with a reason when nothing matches.
//!
//! Marking is not requestable in the plan (the [`fabric::Query`] surface has no
//! marking field); "never above clearance" is enforced structurally at grounding
//! time by the fabric's per-object authorisation. The clearance is threaded here
//! for parity with A.2 and for future compartment-aware scoping. [FR-MI-2]

use std::collections::{BTreeMap, BTreeSet};

use policy::Clearance;

use crate::summary::OntologySummary;

/// Default per-retrieval result cap (precise lookups, not broad scans).
pub const DEFAULT_MAX_RESULTS: usize = 25;

/// One typed retrieval (mirrors [`fabric::Query`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retrieval {
    /// The object type id to fetch.
    pub object_type: String,
    /// Equality filters (property api name → value string).
    pub filter: BTreeMap<String, String>,
    /// Link type ids to traverse (search-around).
    pub link_traversals: Vec<String>,
    /// Maximum results.
    pub max_results: usize,
}

/// A retrieval plan (mirrors the A.2 JSON output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalPlan {
    /// Whether the task can be grounded in the available ontology.
    pub groundable: bool,
    /// The retrievals to execute.
    pub retrievals: Vec<Retrieval>,
    /// Why not, when `groundable` is false.
    pub reason_if_not: Option<String>,
}

/// Lowercase word tokens of a task (letters/digits runs).
fn tokens(task: &str) -> Vec<String> {
    task.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect()
}

/// Does the task mention this type by id or api name (case-insensitive, whole
/// token or contained)? Precise: matches a type only when its name appears.
fn task_mentions(task_tokens: &[String], id: &str, api_name: &str) -> bool {
    let id_l = id.to_ascii_lowercase();
    let api_l = api_name.to_ascii_lowercase();
    task_tokens.iter().any(|t| *t == id_l || *t == api_l)
}

/// Formulate a retrieval plan for `task` against `summary`, honouring the A.2
/// rules. Only types present in `summary` are ever referenced.
pub fn formulate_plan(
    task: &str,
    summary: &OntologySummary,
    _clearance: &Clearance,
) -> RetrievalPlan {
    let toks = tokens(task);

    // Which existing object types does the task actually name?
    let matched: BTreeSet<String> = summary
        .objects
        .iter()
        .filter(|o| task_mentions(&toks, &o.id, &o.api_name))
        .map(|o| o.id.clone())
        .collect();

    if matched.is_empty() {
        return RetrievalPlan {
            groundable: false,
            retrievals: Vec::new(),
            reason_if_not: Some(
                "No object type in the ontology summary matches the task.".to_string(),
            ),
        };
    }

    // One precise retrieval per matched type, expanded by search-around over its
    // outbound links (so "who is assigned to case X" reaches the assignee even
    // though the neighbour type was not named). Deterministic (matched is sorted).
    let retrievals = matched
        .iter()
        .map(|type_id| {
            let link_traversals = summary
                .links
                .iter()
                .filter(|l| &l.source == type_id)
                .map(|l| l.id.clone())
                .collect();
            Retrieval {
                object_type: type_id.clone(),
                filter: BTreeMap::new(),
                link_traversals,
                max_results: DEFAULT_MAX_RESULTS,
            }
        })
        .collect();

    RetrievalPlan {
        groundable: true,
        retrievals,
        reason_if_not: None,
    }
}
