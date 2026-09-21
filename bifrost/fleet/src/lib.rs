//! # bifrost-fleet — declarative channels & desired-vs-actual reconciliation
//!
//! The fleet manager is **declarative**: you set a [`Channel`]'s desired version,
//! and the reconciler reports which [`Installation`]s have drifted and produces the
//! [`bifrost_rollout::RolloutPlan`] that brings them into line. It is the
//! "State reconciler (desired vs actual)" of the SAD, feeding the rollout
//! orchestrator. [FR-B-3]
//!
//! Deterministic and clock-free: reconciliation is a pure function of the recorded
//! desired and actual state.

use std::collections::BTreeMap;

use bifrost_rollout::{RolloutPlan, VersionId};

pub mod agent;
pub mod durable;
pub use agent::AgentReport;
pub use durable::{DurableFleet, FleetIoError};

/// A recorded agent check-in: the version + health an installation last reported,
/// and when the manager last heard from it (its clock). [FR-B-3]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckIn {
    /// The version the agent reported running.
    pub version: VersionId,
    /// Whether the agent reported itself healthy.
    pub healthy: bool,
    /// When the manager last received a check-in from this install (its clock).
    pub last_seen: u64,
}

/// A node's liveness relative to a staleness window. [FR-B-3]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    /// Checked in within the staleness window and reported healthy.
    Live,
    /// Checked in within the window but reported unhealthy.
    Unhealthy,
    /// Has not checked in within the staleness window (went silent).
    Stale,
    /// Never checked in at all.
    Absent,
}

/// A named release stream with a desired version (e.g. `stable`, `edge`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    /// Channel name.
    pub name: String,
    /// The version installations on this channel should converge to.
    pub desired: VersionId,
}

/// One deployed instance in the fleet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    /// Installation id.
    pub id: String,
    /// The channel this installation follows.
    pub channel: String,
    /// The version currently running (actual state).
    pub version: VersionId,
}

impl Installation {
    /// Build an installation record.
    pub fn new(id: impl Into<String>, channel: impl Into<String>, version: VersionId) -> Self {
        Self {
            id: id.into(),
            channel: channel.into(),
            version,
        }
    }
}

/// A detected drift: an installation not on its channel's desired version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    /// The drifted installation.
    pub install_id: String,
    /// Its channel.
    pub channel: String,
    /// The version it is on now.
    pub from: VersionId,
    /// The version it should be on.
    pub to: VersionId,
}

/// The fleet: declarative channel desires plus installation actuals, backed by
/// live agent check-ins. Still clock-free — the caller supplies `now` — but now it
/// records real agent reports and can flag nodes that have gone silent. [FR-B-3]
#[derive(Debug, Default)]
pub struct Fleet {
    channels: BTreeMap<String, VersionId>,
    installs: BTreeMap<String, Installation>,
    checkins: BTreeMap<String, CheckIn>,
}

impl Fleet {
    /// An empty fleet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare (or update) a channel's desired version.
    pub fn set_channel(&mut self, name: impl Into<String>, desired: VersionId) {
        self.channels.insert(name.into(), desired);
    }

    /// Every declared channel and its desired version, by name.
    pub fn channels(&self) -> impl Iterator<Item = (&str, &VersionId)> {
        self.channels.iter().map(|(name, v)| (name.as_str(), v))
    }

    /// Every known installation, by id.
    pub fn installations(&self) -> impl Iterator<Item = &Installation> {
        self.installs.values()
    }

    /// The desired version for a channel, if declared.
    pub fn channel_desired(&self, channel: &str) -> Option<&VersionId> {
        self.channels.get(channel)
    }

    /// Register or replace an installation record.
    pub fn register(&mut self, install: Installation) {
        self.installs.insert(install.id.clone(), install);
    }

    /// Record that an installation now runs `version` (actual state after a deploy).
    pub fn mark_updated(&mut self, install_id: &str, version: VersionId) {
        if let Some(i) = self.installs.get_mut(install_id) {
            i.version = version;
        }
    }

    /// Record an on-host agent's check-in, stamped with the manager's clock `now`.
    /// Updates the installation's actual version (so drift/reconcile see live
    /// state) and the node's last-seen/health. This is what the PQ-mTLS fleet
    /// listener calls for each authenticated check-in. [FR-B-3] [RC-17]
    pub fn check_in(&mut self, report: AgentReport, now: u64) {
        self.register(Installation::new(
            report.install_id.clone(),
            report.channel.clone(),
            report.version.clone(),
        ));
        self.checkins.insert(
            report.install_id,
            CheckIn {
                version: report.version,
                healthy: report.healthy,
                last_seen: now,
            },
        );
    }

    /// The most recent check-in from `install_id`, if any.
    pub fn checkin(&self, install_id: &str) -> Option<&CheckIn> {
        self.checkins.get(install_id)
    }

    /// A node's liveness at `now` given a staleness window `ttl` (a node is stale if
    /// it has not checked in within `ttl` of `now`). An unknown node is `Absent`.
    pub fn node_status(&self, install_id: &str, now: u64, ttl: u64) -> NodeStatus {
        match self.checkins.get(install_id) {
            None => NodeStatus::Absent,
            Some(c) if now.saturating_sub(c.last_seen) > ttl => NodeStatus::Stale,
            Some(c) if !c.healthy => NodeStatus::Unhealthy,
            Some(_) => NodeStatus::Live,
        }
    }

    /// Installations that have checked in but not within the staleness window —
    /// nodes that have gone silent. Deterministic order (by id). [RC-17]
    pub fn stale_nodes(&self, now: u64, ttl: u64) -> Vec<String> {
        self.checkins
            .iter()
            .filter(|(_, c)| now.saturating_sub(c.last_seen) > ttl)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Of the `expected` installation ids, those that are NOT live at `now` — i.e.
    /// stale or never seen. This is how the manager flags absent nodes it expected
    /// to hear from. [RC-17]
    pub fn absent_nodes(&self, expected: &[&str], now: u64, ttl: u64) -> Vec<String> {
        expected
            .iter()
            .filter(|id| {
                !matches!(
                    self.node_status(id, now, ttl),
                    NodeStatus::Live | NodeStatus::Unhealthy
                )
            })
            .map(|id| id.to_string())
            .collect()
    }

    /// Every installation that is not on its channel's desired version. An
    /// installation whose channel has no declared desire is not drifted (nothing to
    /// converge to). Deterministic order (by installation id).
    pub fn drift(&self) -> Vec<Drift> {
        let mut out = Vec::new();
        for install in self.installs.values() {
            if let Some(desired) = self.channels.get(&install.channel) {
                if &install.version != desired {
                    out.push(Drift {
                        install_id: install.id.clone(),
                        channel: install.channel.clone(),
                        from: install.version.clone(),
                        to: desired.clone(),
                    });
                }
            }
        }
        out
    }

    /// Whether every installation matches its channel's desired version.
    pub fn is_converged(&self) -> bool {
        self.drift().is_empty()
    }

    /// Build a canary→broad rollout plan for the drifted installations of a channel.
    /// Returns an empty plan when the channel is already converged (or unknown).
    pub fn reconcile_plan(&self, channel: &str, canary: usize) -> RolloutPlan {
        let drifted = self
            .drift()
            .into_iter()
            .filter(|d| d.channel == channel)
            .count();
        RolloutPlan::canary_broad(drifted, canary)
    }
}
