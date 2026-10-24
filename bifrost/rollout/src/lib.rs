//! # bifrost-rollout — staged, health-gated, zero-downtime rollouts
//!
//! The rollout orchestrator advances a release through ordered rings — a small
//! **canary** first, then the **broad** fleet — health-gating each ring before
//! advancing. Promotion is **blue-green**: the new version comes up alongside the
//! old and only takes over once the ring is healthy, so availability never drops.
//! If any ring's gate fails, every promoted ring is automatically reverted to the
//! prior good version and the rollout reports `RolledBack`. [FR-B-2] [NFR-AVA-1]
//! [AC-B-2]
//!
//! This is the same health-gate-then-rollback discipline as the core `scheduler`'s
//! replica-level rolling upgrade, lifted to fleet-wide ring granularity.
//!
//! [`runs`] is the same discipline for a live fleet, as a clock-free state machine a
//! server steps with agents' reports: releases, channels, per-service rings with a soak
//! and a readiness timeout, one run per channel, and rollback on a failed gate or by
//! hand.

pub mod gate;
pub mod orchestrator;
pub mod plan;
pub mod runs;

pub use gate::HealthGate;
pub use orchestrator::{DeployAgent, DeployError, Orchestrator, RolloutReport, RolloutResult};
pub use plan::{Ring, RolloutPlan, VersionId};
