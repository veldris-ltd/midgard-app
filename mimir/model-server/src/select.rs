//! Autonomous, hardware-aware backend selection.
//!
//! Given the node's [`NodeProfile`] and the registered engines, the selector
//! picks the best engine the node can actually run — deterministically, with a
//! recorded reason, and overridable by an explicit request. This is the machinery
//! that makes GPU-in-Docker serving "switch in" automatically on capable hardware
//! and degrade gracefully everywhere else (§2.a decision; SAD_02b §7 fleet
//! behaviour, pulled forward). [FR-MI-1]

use std::sync::Arc;

use crate::backend::{InferError, ModelBackend};
use crate::hardware::NodeProfile;

/// The outcome of selection: which engine, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The chosen engine id.
    pub backend_id: String,
    /// Human-readable rationale (logged/audited).
    pub reason: String,
    /// The node the decision was made against.
    pub node: NodeProfile,
}

/// Select the serving engine for `node` from `registry`.
///
/// - With `override_id`: that engine is used **iff** it is hardware-eligible;
///   otherwise selection fails closed (an explicit request is never silently
///   downgraded).
/// - Otherwise: the highest-rank eligible engine wins; ties break by lexically
///   smallest id, so selection is deterministic.
pub fn select(
    node: &NodeProfile,
    registry: &[Arc<dyn ModelBackend>],
    override_id: Option<&str>,
) -> Result<(Arc<dyn ModelBackend>, Selection), InferError> {
    if let Some(id) = override_id {
        let be = registry
            .iter()
            .find(|b| b.descriptor().id == id)
            .ok_or_else(|| InferError::PinnedBackendIneligible(id.to_string()))?;
        if !be.descriptor().hw.is_satisfied_by(node) {
            return Err(InferError::PinnedBackendIneligible(id.to_string()));
        }
        return Ok((
            be.clone(),
            Selection {
                backend_id: id.to_string(),
                reason: format!("explicit override to '{id}' (hardware-eligible)"),
                node: *node,
            },
        ));
    }

    // Highest rank among eligible; deterministic tie-break by id.
    let mut best: Option<&Arc<dyn ModelBackend>> = None;
    for be in registry {
        if !be.descriptor().hw.is_satisfied_by(node) {
            continue;
        }
        best = match best {
            None => Some(be),
            Some(cur) => {
                let d = be.descriptor();
                let c = cur.descriptor();
                // Higher rank wins; on a tie, the lexically smaller id wins so the
                // decision is deterministic across runs.
                let better = d.rank > c.rank || (d.rank == c.rank && d.id < c.id);
                if better {
                    Some(be)
                } else {
                    Some(cur)
                }
            }
        };
    }

    match best {
        Some(be) => {
            let d = be.descriptor();
            Ok((
                be.clone(),
                Selection {
                    backend_id: d.id.clone(),
                    reason: format!(
                        "highest-rank eligible engine (rank {}, {} GPU(s) available)",
                        d.rank, node.gpus
                    ),
                    node: *node,
                },
            ))
        }
        None => Err(InferError::NoEligibleBackend),
    }
}
