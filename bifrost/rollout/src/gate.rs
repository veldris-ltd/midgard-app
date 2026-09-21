//! The health gate seam. After a ring is deployed, the orchestrator asks a
//! [`HealthGate`] whether that ring is healthy on the new version before advancing.
//! A real gate reads SLO/health signals from the on-host agents; tests use a
//! deterministic gate. This is the same shape as the core scheduler's `HealthCheck`,
//! lifted to ring granularity. [FR-B-2] [NFR-AVA-1]

use crate::plan::VersionId;

/// Decides whether a ring is healthy running a given version.
pub trait HealthGate {
    /// Return `true` iff `ring` is healthy on `version`.
    fn healthy(&self, ring: &str, version: &VersionId) -> bool;
}
