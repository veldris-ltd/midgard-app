//! The decision-support adapter. It assembles the A.8 prompt, calls the Mimir
//! option-provider seam, parses the ranked option set, and **grounds every option's
//! citations against the live picture**. The result is [`AdvisoryOptions`]: cited,
//! ranked, and strictly advisory — it exposes no way to act. A human authorises any
//! consequential action through `heimdall-workflow`. [FR-H-4] [NFR-AI-1] [AC-H-3]

use std::collections::BTreeSet;

use heimdall_picture::Picture;

use crate::error::DecisionError;
use crate::options::{Citation, CitationKind, OptionSet};
use crate::prompt::assemble;
use crate::provider::OptionProvider;

/// How well one option's citations resolve against the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionGrounding {
    /// The option's rank.
    pub rank: u32,
    /// True iff the option cites at least one item and *every* citation resolves to
    /// a real picture element (no invented evidence). [FR-H-4]
    pub grounded: bool,
    /// Citations that did not resolve (invented or malformed).
    pub unresolved: Vec<Citation>,
}

/// The advisory result of a decision-support request: the parsed option set plus its
/// grounding against the picture. **Advisory by construction** — there is no method
/// that executes an option; acting requires the audited human-gated workflow.
/// [NFR-AI-1] [AC-H-3]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryOptions {
    /// The parsed, ranked options.
    pub option_set: OptionSet,
    /// Per-option grounding, aligned to `option_set.options`.
    pub grounding: Vec<OptionGrounding>,
}

impl AdvisoryOptions {
    /// Always `true`: decision support proposes; it never acts. This is the
    /// structural counterpart to Forge's uncommitted preview. [NFR-AI-1]
    pub fn is_advisory(&self) -> bool {
        true
    }

    /// Whether every returned option is grounded (cited and fully resolved).
    pub fn all_grounded(&self) -> bool {
        !self.grounding.is_empty() && self.grounding.iter().all(|g| g.grounded)
    }

    /// The grounding for an option by rank.
    pub fn grounding_for(&self, rank: u32) -> Option<&OptionGrounding> {
        self.grounding.iter().find(|g| g.rank == rank)
    }
}

/// Ask Mimir (via `provider`) for ranked, cited options addressing `decision_point`
/// over `picture`. The result is always advisory and every option is grounded
/// against the picture. Fails closed if inference fails or the response is
/// malformed. [FR-H-4] [AC-H-3]
pub fn request_options<P: OptionProvider + ?Sized>(
    picture: &Picture,
    decision_point: &str,
    provider: &P,
) -> Result<AdvisoryOptions, DecisionError> {
    let prompt = assemble(picture, decision_point);
    let raw = provider.generate(&prompt)?;
    let option_set = OptionSet::parse(&raw)?;

    // The grounding sets: valid entity ids and the feeds that actually contributed.
    let entity_ids: BTreeSet<&str> = picture.entities().map(|e| e.id.0.as_str()).collect();
    let feed_ids: BTreeSet<&str> = picture
        .entities()
        .flat_map(|e| e.sources.iter().map(|s| s.as_str()))
        .collect();

    let grounding = option_set
        .options
        .iter()
        .map(|opt| {
            let unresolved: Vec<Citation> = opt
                .citations
                .iter()
                .filter(|c| !resolves(c, &entity_ids, &feed_ids))
                .cloned()
                .collect();
            OptionGrounding {
                rank: opt.rank,
                grounded: !opt.citations.is_empty() && unresolved.is_empty(),
                unresolved,
            }
        })
        .collect();

    Ok(AdvisoryOptions {
        option_set,
        grounding,
    })
}

/// Whether a citation resolves to a real picture element.
fn resolves(c: &Citation, entity_ids: &BTreeSet<&str>, feed_ids: &BTreeSet<&str>) -> bool {
    match c.kind {
        // A track shares its entity's id in this phase.
        CitationKind::Entity | CitationKind::Track => entity_ids.contains(c.id.as_str()),
        CitationKind::Feed => feed_ids.contains(c.id.as_str()),
        CitationKind::Unknown => false,
    }
}
