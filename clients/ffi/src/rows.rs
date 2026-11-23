//! Rows as the shells get them: each one shown in full (its typed read model, marking
//! chip and, for an audit row, its signature status) or withheld by level only.
//! `[FR-C-1]` `[AC-C-13]` `[NFR-PQ-3]`

use midgard_client_core::consoles as core;
use midgard_client_core::keystore::{Keystore, TrustRoot};
use midgard_client_core::marking::{MarkingLevel, RowMarking, Visible};
use midgard_client_core::session::Session;
use midgard_client_core::source::DataSource;

use crate::client::{MarkingChip, MarkingLevelChip};
use crate::ClientError;

/// What an audit row's signature check found, as the session decided it (a source's
/// own claim is never passed on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SignatureStatus {
    /// Verified under the pinned demo-audit anchor, in the core: show a check mark.
    Verified,
    /// Signed by this device's audit key for one of its own actions, and verified
    /// under it: show a check mark.
    Device,
    /// Nothing checked: no check mark.
    Unsigned,
}

impl SignatureStatus {
    /// From the status word the session writes into every audit row's `pqSignature`.
    fn of(word: &str) -> Self {
        match word {
            "verified" => SignatureStatus::Verified,
            "device" => SignatureStatus::Device,
            _ => SignatureStatus::Unsigned,
        }
    }
}

/// One row of a collection.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ConsoleRow {
    /// Position in the collection.
    pub position: u32,
    /// `None` when withheld.
    pub id: Option<String>,
    /// The marking chip; a withheld row's names its level only.
    pub marking: MarkingChip,
    /// Above the session's clearance.
    pub withheld: bool,
    /// For an audit row (`AuditRecord`) only; `None` for every other row and every
    /// withheld row.
    pub signature: Option<SignatureStatus>,
    /// The row, typed; `None` when withheld.
    pub item: Option<RowItem>,
}

pub(crate) fn level_chip(l: MarkingLevel) -> MarkingLevelChip {
    match l {
        MarkingLevel::Open => MarkingLevelChip::Open,
        MarkingLevel::Internal => MarkingLevelChip::Internal,
        MarkingLevel::Restricted => MarkingLevelChip::Restricted,
        MarkingLevel::Confidential => MarkingLevelChip::Confidential,
    }
}

pub(crate) fn chip(m: &RowMarking) -> MarkingChip {
    MarkingChip {
        level: level_chip(m.level()),
        label: m.label(),
    }
}

fn row(position: usize, v: Visible<RowItem>) -> ConsoleRow {
    let position = u32::try_from(position).unwrap_or(u32::MAX);
    match v {
        Visible::Shown { id, marking, item } => ConsoleRow {
            position,
            id: Some(id),
            marking: chip(&marking),
            withheld: false,
            signature: match &item {
                RowItem::AuditRecord { item } => Some(SignatureStatus::of(&item.pq_signature)),
                _ => None,
            },
            item: Some(item),
        },
        Visible::Withheld { level } => ConsoleRow {
            position,
            id: None,
            marking: MarkingChip {
                level: level_chip(level),
                label: level.as_str().to_uppercase(),
            },
            withheld: true,
            signature: None,
            item: None,
        },
    }
}

macro_rules! row_items {
    ($($module:ident :: $ty:ident),* $(,)?) => {
        /// A row's read model, one variant per type a collection may declare
        /// (`midgard_client_core::consoles::ROW_TYPES`).
        #[derive(Debug, Clone, PartialEq, uniffi::Enum)]
        #[allow(clippy::large_enum_variant)]
        pub enum RowItem {
            $(
                $ty {
                    /// The row.
                    item: crate::generated::$module::$ty,
                },
            )*
        }

        /// The read-model type names [`RowItem`] covers.
        pub fn row_types() -> Vec<&'static str> {
            vec![$(stringify!($ty)),*]
        }

        /// The rows of `collection`, whose read model is `type_name`, decoded and
        /// clearance-filtered by the session.
        pub(crate) fn typed_rows<K: Keystore + TrustRoot, S: DataSource>(
            s: &mut Session<K, S>,
            type_name: &str,
            collection: &str,
        ) -> Result<Vec<ConsoleRow>, ClientError> {
            let rows: Vec<Visible<RowItem>> = match type_name {
                $(
                    stringify!($ty) => s
                        .rows::<core::$module::$ty>(collection)?
                        .into_iter()
                        .map(|v| v.map(|x| RowItem::$ty { item: x.into() }))
                        .collect(),
                )*
                other => {
                    return Err(ClientError::Malformed {
                        detail: format!("{collection}: no binding for row type {other:?}"),
                    })
                }
            };
            Ok(rows.into_iter().enumerate().map(|(i, v)| row(i, v)).collect())
        }
    };
}

// The same list as the core's registry (`consoles/mod.rs`); the test below keeps
// the two equal.
row_items! {
    admin::AdminApproval,
    admin::AdminBinding,
    admin::AdminBreakGlass,
    admin::AdminConfigCommit,
    admin::AdminConfigSimulation,
    admin::AdminCopilotTurn,
    admin::AdminLedgerEntry,
    admin::AdminOverview,
    admin::AdminRegistryItem,
    admin::AdminRole,
    common::AuditRecord,
    bifrost::BifrostArtefact,
    bifrost::BifrostBundle,
    bifrost::BifrostChannel,
    bifrost::BifrostFleetNode,
    bifrost::BifrostRelease,
    bifrost::BifrostRing,
    bifrost::BifrostRollback,
    forge::ForgeConnector,
    forge::ForgeDataset,
    forge::ForgeHealthCheck,
    forge::ForgePipeline,
    forge::ForgeProjection,
    forge::ForgeSchedule,
    forge::ForgeTransform,
    heimdall::HeimdallDecision,
    heimdall::HeimdallEntity,
    heimdall::HeimdallFeed,
    motion::HeimdallMotion,
    heimdall::HeimdallPicture,
    heimdall::HeimdallSyncSite,
    heimdall::HeimdallTrack,
    heimdall::HeimdallWorkflow,
    midgard::MidgardApproval,
    midgard::MidgardComputeJob,
    midgard::MidgardIdentity,
    midgard::MidgardKeyRecord,
    midgard::MidgardObjectInstance,
    midgard::MidgardObjectType,
    midgard::MidgardPolicyRule,
    midgard::MidgardRolloutState,
    midgard::MidgardRpcSurface,
    midgard::MidgardSearchGraph,
    midgard::MidgardSloMetric,
    mimir::MimirAgent,
    mimir::MimirEval,
    mimir::MimirFeedbackItem,
    mimir::MimirGuardrail,
    mimir::MimirLogicFn,
    mimir::MimirModel,
    mimir::MimirSession,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_core_row_type_has_a_binding_and_no_other() {
        let mut ours = row_types();
        let mut core: Vec<&str> = core::ROW_TYPES.to_vec();
        ours.sort_unstable();
        core.sort_unstable();
        assert_eq!(ours, core);
    }

    #[test]
    fn signature_words_map_and_anything_else_is_unsigned() {
        assert_eq!(SignatureStatus::of("verified"), SignatureStatus::Verified);
        assert_eq!(SignatureStatus::of("device"), SignatureStatus::Device);
        assert_eq!(SignatureStatus::of("none"), SignatureStatus::Unsigned);
        assert_eq!(SignatureStatus::of("ML-DSA ✓"), SignatureStatus::Unsigned);
    }
}
