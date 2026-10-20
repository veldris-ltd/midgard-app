//! Grounding execution: run a [`RetrievalPlan`] through the fabric and bind a
//! citation to every retrieved object.
//!
//! Retrieval goes through the fabric's governed query path, which authorises each
//! candidate object against the caller's clearance/purpose. So the grounding a
//! caller sees **never contains data above their clearance** — the invariant is
//! enforced by the fabric, not re-implemented here. Every fact carries an
//! `obj:<id>` citation, which is what makes AC-MI-2 (answers cite the evidence
//! they used) achievable end to end. [FR-MI-2] [NFR-SEC-2]

use fabric::Query;

use crate::plan::RetrievalPlan;
use crate::{GroundingSource, PolicyContext};

/// One cited piece of evidence retrieved from the fabric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundingFact {
    /// The citation id, `obj:<id>`.
    pub citation: String,
    /// The object type id.
    pub object_type: String,
    /// Rendered properties (name, canonical value), in deterministic order.
    pub properties: Vec<(String, String)>,
    /// The object's classification level (for observability; the fact is already
    /// clearance-authorised by the fabric).
    pub marking_level: u8,
    /// Relevance score; higher is more relevant. Drives OAG truncation under a
    /// token budget (highest-relevance facts are kept). [NFR-PERF-1]
    pub relevance: u32,
}

impl GroundingFact {
    /// A one-line rendering for the GROUNDING assembly layer.
    pub fn line(&self) -> String {
        let props = self
            .properties
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{}] {} {{ {} }}", self.citation, self.object_type, props)
    }
}

/// The cited evidence retrieved for a task.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grounding {
    /// Facts, ordered by descending relevance.
    pub facts: Vec<GroundingFact>,
    /// The audit sequence numbers of the retrieval queries (PQ-signed lineage).
    pub audit_seqs: Vec<u64>,
}

impl Grounding {
    /// Whether any evidence was grounded.
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
    /// The number of grounded facts.
    pub fn len(&self) -> usize {
        self.facts.len()
    }
    /// All citation ids, highest relevance first.
    pub fn citations(&self) -> Vec<String> {
        self.facts.iter().map(|f| f.citation.clone()).collect()
    }
    /// Render the GROUNDING block.
    pub fn render(&self) -> String {
        self.facts
            .iter()
            .map(|f| f.line())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Execute `plan` against `src` under `ctx`, producing cited grounding. Facts are
/// deduplicated by id and ranked by retrieval/result order (earlier = more
/// relevant). A retrieval that errors is skipped (fail-soft on grounding: less
/// evidence, never wrong evidence).
pub fn ground(
    plan: &RetrievalPlan,
    src: &mut dyn GroundingSource,
    ctx: &PolicyContext,
    now: u64,
) -> Grounding {
    let mut facts: Vec<GroundingFact> = Vec::new();
    let mut audit_seqs: Vec<u64> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // Monotonic rank; the first object retrieved is the most relevant.
    let mut rank: u32 = 0;

    for (i, r) in plan.retrievals.iter().enumerate() {
        let q = Query {
            object_type: r.object_type.clone(),
            filter: r.filter.clone(),
            link_traversals: r.link_traversals.clone(),
            max_results: r.max_results,
            search: None,
        };
        let retrieved = match src.retrieve(ctx, &q, now + i as u64) {
            Ok(r) => r,
            Err(_) => continue,
        };
        audit_seqs.push(retrieved.audit_seq);
        for obj in retrieved.objects {
            if !seen.insert(obj.id.clone()) {
                continue;
            }
            let mut properties: Vec<(String, String)> = obj
                .properties
                .iter()
                .map(|(k, v)| (k.clone(), v.as_index_key()))
                .collect();
            properties.sort();
            facts.push(GroundingFact {
                citation: format!("obj:{}", obj.id),
                object_type: obj.type_id.clone(),
                properties,
                marking_level: obj.marking.level.0,
                // Relevance decreases with rank; keep it non-negative.
                relevance: u32::MAX - rank,
            });
            rank += 1;
        }
    }

    // Highest relevance first (stable — matches retrieval order).
    facts.sort_by(|a, b| b.relevance.cmp(&a.relevance));
    Grounding { facts, audit_seqs }
}
