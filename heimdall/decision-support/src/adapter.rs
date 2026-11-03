//! The decision-support adapter. It assembles the A.8 prompt, calls the Mimir
//! option-provider seam, parses the ranked option set, and **grounds every option's
//! citations against the live picture**. The result is [`AdvisoryOptions`]: only
//! options whose every citation resolved are offered, re-ranked `1..n`; the rest are
//! returned separately as rejected, with the citations that did not resolve. It is
//! strictly advisory — it exposes no way to act. A human authorises any action
//! through `heimdall-workflow`. [FR-H-4] [NFR-AI-1] [AC-H-3] [VLD-ANB-MIDGARD-0079]

use std::collections::BTreeSet;

use heimdall_picture::Picture;

use crate::error::DecisionError;
use crate::options::{Citation, CitationKind, CourseOfAction, GroundedOption, OptionSet};
use crate::prompt::assemble;
use crate::provider::OptionProvider;

/// An option the adapter refused to offer: it cited nothing, or cited evidence that
/// does not resolve against the picture. Returned, not discarded, so operators and
/// evals can see what the model invented. [VLD-ANB-MIDGARD-0079]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedOption {
    /// The option as the model gave it (its original rank).
    pub option: CourseOfAction,
    /// The citations that did not resolve (empty when the option cited nothing).
    pub unresolved: Vec<Citation>,
}

/// The advisory result of a decision-support request. **Advisory by
/// construction** — there is no method that executes an option; acting requires
/// the audited human-gated workflow. [NFR-AI-1] [AC-H-3]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryOptions {
    /// The grounded options only, re-ranked `1..n` in the model's order, with the
    /// model's `unsupported` reason. [VLD-ANB-MIDGARD-0079]
    pub option_set: OptionSet,
    /// The same grounded options as [`GroundedOption`]s — the only input
    /// `heimdall-workflow` accepts to build a proposed action.
    pub grounded: Vec<GroundedOption>,
    /// Options refused because they cited nothing or cited evidence that does not
    /// resolve, each with its unresolved citations.
    pub rejected: Vec<RejectedOption>,
}

impl AdvisoryOptions {
    /// Always `true`: decision support proposes; it never acts. This is the
    /// structural counterpart to Forge's uncommitted preview. [NFR-AI-1]
    pub fn is_advisory(&self) -> bool {
        true
    }

    /// Whether the model's every option was grounded: at least one option is
    /// offered and none was rejected.
    pub fn all_grounded(&self) -> bool {
        !self.grounded.is_empty() && self.rejected.is_empty()
    }

    /// The grounded option at `rank` (in the re-ranked, grounded set).
    pub fn grounding_for(&self, rank: u32) -> Option<&GroundedOption> {
        self.grounded.iter().find(|g| g.rank() == rank)
    }
}

/// Ask Mimir (via `provider`) for ranked, cited options addressing `decision_point`
/// over `picture`. The result is always advisory; every option is grounded against
/// the picture, and only grounded ones are offered. Fails closed if inference fails
/// or the response is malformed (including duplicate or gapped ranks).
/// [FR-H-4] [AC-H-3] [VLD-ANB-MIDGARD-0079]
pub fn request_options<P: OptionProvider>(
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

    // An option is grounded iff it cites at least one item and every citation
    // resolves. Grounded options keep the model's order and are re-ranked 1..n.
    let mut grounded = Vec::new();
    let mut rejected = Vec::new();
    for opt in option_set.options {
        let unresolved: Vec<Citation> = opt
            .citations
            .iter()
            .filter(|c| !resolves(c, &entity_ids, &feed_ids))
            .cloned()
            .collect();
        if opt.citations.is_empty() || !unresolved.is_empty() {
            rejected.push(RejectedOption {
                option: opt,
                unresolved,
            });
        } else {
            let rank = grounded.len() as u32 + 1;
            grounded.push(GroundedOption::new(CourseOfAction { rank, ..opt }));
        }
    }

    Ok(AdvisoryOptions {
        option_set: OptionSet {
            options: grounded
                .iter()
                .cloned()
                .map(GroundedOption::into_inner)
                .collect(),
            unsupported: option_set.unsupported,
        },
        grounded,
        rejected,
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
