//! Live rollouts: releases, channels and ring-by-ring runs driven by agents' reports.
//! [FR-B-2] [FR-B-3] [NFR-AVA-1]
//!
//! [`Orchestrator`](crate::Orchestrator) rolls a plan out in one call against a gate
//! that answers at once. A real fleet answers over minutes: an agent installs a build,
//! restarts the service and reports back on its next check-in. [`Rollouts`] is the same
//! ring planner, health gate and rollback controller as a state machine that a server
//! steps with a clock and the latest reports ([`Rollouts::tick`]). It does no I/O and
//! reads no clock, so a test drives it with fake time and fake reports, and a server
//! persists it whole and resumes it after a restart.
//!
//! ## Model
//! - A [`Release`] is a version and, for every service the rings name, the artefact
//!   version it ships (the artefact id is the service's name). Releases are immutable.
//! - A [`ChannelState`] is a channel's current release (the last one that fully passed,
//!   or the one it was seeded with) and the one before it.
//! - A [`RingSpec`] is an ordered group of services with a soak time and a readiness
//!   timeout ([`RingConfig::suite_default`]: canary forge-worker, then midgard-replay,
//!   heimdall-feed and heimdall-source, then mimir-server, then http-gateway last).
//! - A [`RolloutRun`] takes one channel from its current release to another, ring by
//!   ring. One run per channel is active at a time.
//!
//! ## Desired versions
//! While a run is active, each service in a ring that has started (waiting, soaking or
//! passed) is desired at the target release's version, and every other service at the
//! channel's current release's. A run that completes makes the target current; a run
//! that is rolled back leaves the current release as it was, so every service it had
//! changed is desired back at the previous version at once.
//!
//! ## The gate
//! The participants of a channel are the installations that reported per-service status
//! on it within the run's report window (`report_ttl_secs`). A ring is **ready** when
//! there is at least one participant and every participant reports every service of the
//! ring at its target version and healthy. Then:
//! - the ring **soaks**: when it has stayed ready for `soak_secs`, it passes and the
//!   next ring starts (the last one passing completes the run);
//! - it **fails** if it is not ready by its deadline (`ready_timeout_secs` after it
//!   started), or if a service that has reported ready on the target then reports
//!   unhealthy or another version. A failure rolls the run back with the reason.
//! - A ring that loses readiness only because a participant went quiet (or a new one
//!   joined) stops soaking and waits again, with its deadline pushed to at least
//!   `ready_timeout_secs` from then. This is also what a restart looks like: the server
//!   keeps the run but not the reports, so a soaking ring soaks again from the next
//!   reports instead of failing.
//!
//! A manual [`Rollouts::rollback`] rolls an active run back the same way, or, with no
//! run active, moves the channel back to its previous release (or a named one).

use std::collections::{BTreeMap, BTreeSet};

/// The default soak: how long a ring must stay ready before it passes.
pub const DEFAULT_SOAK_SECS: u64 = 60;
/// The default readiness timeout: how long a ring has to become ready.
pub const DEFAULT_READY_TIMEOUT_SECS: u64 = 180;
/// The default report window: a report older than this does not count.
pub const DEFAULT_REPORT_TTL_SECS: u64 = 30;
/// Finished runs kept; the oldest are dropped beyond this. Active runs are never dropped.
pub const MAX_FINISHED_RUNS: usize = 200;

/// One ring of a live rollout: services that move together, gated together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingSpec {
    /// The ring's name (`canary`, `ring-2`, …).
    pub name: String,
    /// The services in it, by name (each is also its artefact id).
    pub services: Vec<String>,
    /// How long the ring must stay ready before it passes.
    pub soak_secs: u64,
    /// How long the ring has, from its start, to become ready.
    pub ready_timeout_secs: u64,
}

impl RingSpec {
    /// A ring with the default soak and readiness timeout.
    pub fn new(name: &str, services: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            services: services.iter().map(|s| s.to_string()).collect(),
            soak_secs: DEFAULT_SOAK_SECS,
            ready_timeout_secs: DEFAULT_READY_TIMEOUT_SECS,
        }
    }
}

/// The rings, in order, and the report window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingConfig {
    /// Rings in rollout order.
    pub rings: Vec<RingSpec>,
    /// A report older than this many seconds does not count towards a gate.
    pub report_ttl_secs: u64,
}

impl RingConfig {
    /// The suite on one Mac: canary forge-worker; ring-2 midgard-replay, heimdall-feed
    /// and heimdall-source; ring-3 mimir-server; last http-gateway (it carries the
    /// consoles). Soak 60 s, readiness timeout 180 s, report window 30 s.
    pub fn suite_default() -> Self {
        Self {
            rings: vec![
                RingSpec::new("canary", &["forge-worker"]),
                RingSpec::new(
                    "ring-2",
                    &["midgard-replay", "heimdall-feed", "heimdall-source"],
                ),
                RingSpec::new("ring-3", &["mimir-server"]),
                RingSpec::new("last", &["http-gateway"]),
            ],
            report_ttl_secs: DEFAULT_REPORT_TTL_SECS,
        }
    }

    /// Every service the rings name, in rollout order.
    pub fn services(&self) -> Vec<&str> {
        self.rings
            .iter()
            .flat_map(|r| r.services.iter().map(String::as_str))
            .collect()
    }

    /// Refuse a configuration a rollout could not run: no rings, an empty ring, a ring
    /// or service named twice, a bad name, or a zero readiness timeout or report window.
    pub fn validate(&self) -> Result<(), String> {
        if self.rings.is_empty() {
            return Err("there must be at least one ring".into());
        }
        if self.report_ttl_secs == 0 {
            return Err("the report window must be at least 1 s".into());
        }
        let mut rings = BTreeSet::new();
        let mut services = BTreeSet::new();
        for ring in &self.rings {
            check_name(&ring.name, "a ring name")?;
            if !rings.insert(ring.name.as_str()) {
                return Err(format!("ring \"{}\" appears twice", ring.name));
            }
            if ring.services.is_empty() {
                return Err(format!("ring \"{}\" has no services", ring.name));
            }
            if ring.ready_timeout_secs == 0 {
                return Err(format!(
                    "ring \"{}\": the readiness timeout must be at least 1 s",
                    ring.name
                ));
            }
            for service in &ring.services {
                check_name(service, "a service name")?;
                if !services.insert(service.as_str()) {
                    return Err(format!("service \"{service}\" is in more than one ring"));
                }
            }
        }
        Ok(())
    }
}

/// Letters, digits, `.`, `_`, `-` and `+`, 1 to 128 of them: release versions, ring,
/// service and channel names.
pub fn check_name(name: &str, what: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
    {
        return Err(format!(
            "{what} must be 1 to 128 letters, digits, '.', '_', '-' or '+', not {name:?}"
        ));
    }
    Ok(())
}

/// A release: a version and the artefact version of every service in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The release version, e.g. `0.2.0`.
    pub version: String,
    /// Service (= artefact id) to artefact version.
    pub artefacts: BTreeMap<String, String>,
    /// Who created it.
    pub created_by: String,
    /// When (server clock, seconds).
    pub created_at: u64,
}

/// A channel: its current release and the one before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelState {
    /// The channel's name.
    pub name: String,
    /// The current release's version.
    pub current: String,
    /// The release before it, if any.
    pub previous: Option<String>,
}

/// Where a ring of a run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingPhase {
    /// Not started.
    Pending,
    /// Started; waiting for every service to report the target version, healthy.
    Waiting,
    /// Ready; soaking.
    Soaking,
    /// Soaked; passed.
    Passed,
    /// Its gate failed.
    Failed,
    /// It had started (or passed) and the run was rolled back.
    RolledBack,
}

impl RingPhase {
    /// The phase's name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            RingPhase::Pending => "pending",
            RingPhase::Waiting => "waiting",
            RingPhase::Soaking => "soaking",
            RingPhase::Passed => "passed",
            RingPhase::Failed => "failed",
            RingPhase::RolledBack => "rolled-back",
        }
    }

    /// The phase named `s`.
    pub fn parse(s: &str) -> Option<Self> {
        [
            RingPhase::Pending,
            RingPhase::Waiting,
            RingPhase::Soaking,
            RingPhase::Passed,
            RingPhase::Failed,
            RingPhase::RolledBack,
        ]
        .into_iter()
        .find(|p| p.as_str() == s)
    }

    /// Whether the ring's services are desired at the target version.
    fn on_target(self) -> bool {
        matches!(
            self,
            RingPhase::Waiting | RingPhase::Soaking | RingPhase::Passed
        )
    }
}

/// One ring of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingRun {
    /// The ring as it was configured when the run started.
    pub spec: RingSpec,
    /// Where it is.
    pub phase: RingPhase,
    /// When it started.
    pub started_at: Option<u64>,
    /// When it must be ready by (while waiting).
    pub deadline: Option<u64>,
    /// Since when it has been ready (while soaking).
    pub soak_since: Option<u64>,
    /// When it passed, failed or was rolled back.
    pub ended_at: Option<u64>,
    /// Why it failed.
    pub reason: Option<String>,
    /// `install/service` pairs that have reported ready on the target.
    pub ready_seen: BTreeSet<String>,
}

impl RingRun {
    fn new(spec: RingSpec) -> Self {
        Self {
            spec,
            phase: RingPhase::Pending,
            started_at: None,
            deadline: None,
            soak_since: None,
            ended_at: None,
            reason: None,
            ready_seen: BTreeSet::new(),
        }
    }

    fn start(&mut self, now: u64) {
        self.phase = RingPhase::Waiting;
        self.started_at = Some(now);
        self.deadline = Some(now.saturating_add(self.spec.ready_timeout_secs));
    }
}

/// What kind of run: a promotion through the rings, or a manual rollback of a channel
/// with no promotion in progress (which finishes at once).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    /// A promotion, ring by ring.
    Promote,
    /// A manual rollback to an earlier release.
    Rollback,
}

impl RunKind {
    /// The kind's name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            RunKind::Promote => "promote",
            RunKind::Rollback => "rollback",
        }
    }
    /// The kind named `s`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "promote" => Some(RunKind::Promote),
            "rollback" => Some(RunKind::Rollback),
            _ => None,
        }
    }
}

/// Where a run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// In progress.
    Running,
    /// Every ring passed; the target is the channel's current release.
    Completed,
    /// A gate failed or someone rolled it back; the channel stayed (or went back) on the
    /// previous release.
    RolledBack,
}

impl RunState {
    /// The state's name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Running => "running",
            RunState::Completed => "completed",
            RunState::RolledBack => "rolled-back",
        }
    }
    /// The state named `s`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "running" => Some(RunState::Running),
            "completed" => Some(RunState::Completed),
            "rolled-back" => Some(RunState::RolledBack),
            _ => None,
        }
    }
}

/// One rollout of a release to a channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutRun {
    /// `run-<n>`.
    pub id: String,
    /// Promotion or manual rollback.
    pub kind: RunKind,
    /// The channel.
    pub channel: String,
    /// The release it moves to.
    pub release: String,
    /// The release the channel was on.
    pub from: String,
    /// The rings, as configured when it started (none for a manual rollback).
    pub rings: Vec<RingRun>,
    /// The report window it gates with.
    pub report_ttl_secs: u64,
    /// Where it is.
    pub state: RunState,
    /// When it started.
    pub started_at: u64,
    /// Who started it.
    pub started_by: String,
    /// When it finished.
    pub ended_at: Option<u64>,
    /// Who finished it: the server's orchestrator for a gate, or the identity that
    /// rolled it back.
    pub ended_by: Option<String>,
    /// Why it was rolled back.
    pub reason: Option<String>,
}

impl RolloutRun {
    /// The ring in progress, if any.
    pub fn current_ring(&self) -> Option<usize> {
        self.rings
            .iter()
            .position(|r| matches!(r.phase, RingPhase::Waiting | RingPhase::Soaking))
    }
}

/// A service as an installation reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatus {
    /// The version it runs.
    pub version: String,
    /// Whether it is up and ready.
    pub healthy: bool,
}

/// An installation's latest per-service report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    /// The installation.
    pub install_id: String,
    /// Its channel.
    pub channel: String,
    /// When the server received it.
    pub at: u64,
    /// Each service it runs.
    pub services: BTreeMap<String, ServiceStatus>,
}

/// Something that happened, for the audit trail and the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A run started.
    RunStarted {
        run: String,
        channel: String,
        from: String,
        release: String,
    },
    /// A ring started: its services are now desired at the target.
    RingStarted { run: String, ring: String },
    /// A ring became ready and started soaking.
    RingReady { run: String, ring: String },
    /// A ring passed its soak.
    RingPassed { run: String, ring: String },
    /// A ring's gate failed.
    RingFailed {
        run: String,
        ring: String,
        reason: String,
    },
    /// Every ring passed.
    RunCompleted {
        run: String,
        channel: String,
        release: String,
    },
    /// The run (or the channel) went back to `to`.
    RolledBack {
        run: String,
        channel: String,
        to: String,
        reason: String,
    },
}

impl Event {
    /// The audit action for it.
    pub fn action(&self) -> String {
        match self {
            Event::RunStarted {
                channel,
                from,
                release,
                ..
            } => format!("Rollout started: {channel} {from} -> {release}"),
            Event::RingStarted { ring, .. } => format!("Rollout ring {ring} started"),
            Event::RingReady { ring, .. } => format!("Rollout ring {ring} ready, soaking"),
            Event::RingPassed { ring, .. } => format!("Rollout ring {ring} passed"),
            Event::RingFailed { ring, reason, .. } => {
                format!("Rollout ring {ring} failed: {reason}")
            }
            Event::RunCompleted {
                channel, release, ..
            } => format!("Rollout completed: {channel} is on {release}"),
            Event::RolledBack {
                channel,
                to,
                reason,
                ..
            } => format!("Rollout rolled back: {channel} back to {to}: {reason}"),
        }
    }

    /// The run it is about.
    pub fn run(&self) -> &str {
        match self {
            Event::RunStarted { run, .. }
            | Event::RingStarted { run, .. }
            | Event::RingReady { run, .. }
            | Event::RingPassed { run, .. }
            | Event::RingFailed { run, .. }
            | Event::RunCompleted { run, .. }
            | Event::RolledBack { run, .. } => run,
        }
    }
}

/// Why an operation was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RolloutError {
    /// No such release, channel or run.
    NotFound(String),
    /// It clashes with the state: the release exists, a run is active, it is already
    /// current, there is nothing to roll back to.
    Conflict(String),
    /// The request itself is wrong.
    Invalid(String),
}

impl std::fmt::Display for RolloutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RolloutError::NotFound(m) | RolloutError::Conflict(m) | RolloutError::Invalid(m) => {
                f.write_str(m)
            }
        }
    }
}

impl std::error::Error for RolloutError {}

/// What a channel's installations should run now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desired {
    /// The channel's current release.
    pub release: String,
    /// The active run, if any.
    pub run: Option<String>,
    /// The release the active run moves to, if any.
    pub target: Option<String>,
    /// Each service's desired artefact version.
    pub services: BTreeMap<String, String>,
}

/// The whole orchestrator state: releases, channels and runs. Persist it whole.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rollouts {
    /// Releases by version.
    pub releases: BTreeMap<String, Release>,
    /// Channels by name.
    pub channels: BTreeMap<String, ChannelState>,
    /// Runs, oldest first.
    pub runs: Vec<RolloutRun>,
    /// The number of the next run.
    pub next_run: u64,
}

/// The actor recorded for what the orchestrator itself decides.
pub const ORCHESTRATOR: &str = "bifrost-orchestrator";

impl Rollouts {
    /// No releases, channels or runs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a channel on `current` if it is not known yet. Returns whether it was
    /// added. A known channel is left alone: after the first start the orchestrator,
    /// not a seed file, decides where a channel is.
    pub fn seed_channel(&mut self, name: &str, current: &str) -> Result<bool, RolloutError> {
        check_name(name, "a channel name").map_err(RolloutError::Invalid)?;
        check_name(current, "a release version").map_err(RolloutError::Invalid)?;
        if self.channels.contains_key(name) {
            return Ok(false);
        }
        self.channels.insert(
            name.to_string(),
            ChannelState {
                name: name.to_string(),
                current: current.to_string(),
                previous: None,
            },
        );
        Ok(true)
    }

    /// Add a release. It must name an artefact version for exactly the services the
    /// rings name; a version already used is a conflict. (Whether those artefacts are
    /// verified is the registry's to say, before this.)
    pub fn create_release(
        &mut self,
        release: Release,
        config: &RingConfig,
    ) -> Result<(), RolloutError> {
        check_name(&release.version, "a release version").map_err(RolloutError::Invalid)?;
        if self.releases.contains_key(&release.version) {
            return Err(RolloutError::Conflict(format!(
                "release {} already exists",
                release.version
            )));
        }
        let wanted: BTreeSet<&str> = config.services().into_iter().collect();
        let have: BTreeSet<&str> = release.artefacts.keys().map(String::as_str).collect();
        let missing: Vec<&str> = wanted.difference(&have).copied().collect();
        let extra: Vec<&str> = have.difference(&wanted).copied().collect();
        if !missing.is_empty() || !extra.is_empty() {
            let mut problems = Vec::new();
            if !missing.is_empty() {
                problems.push(format!("no artefact for {}", missing.join(", ")));
            }
            if !extra.is_empty() {
                problems.push(format!("{} in no ring", extra.join(", ")));
            }
            return Err(RolloutError::Invalid(format!(
                "release {}: {}",
                release.version,
                problems.join("; ")
            )));
        }
        for version in release.artefacts.values() {
            check_name(version, "an artefact version").map_err(RolloutError::Invalid)?;
        }
        self.releases.insert(release.version.clone(), release);
        Ok(())
    }

    /// The active run on `channel`, if any.
    pub fn active_run(&self, channel: &str) -> Option<&RolloutRun> {
        self.runs
            .iter()
            .find(|r| r.channel == channel && r.state == RunState::Running)
    }

    /// The run `id`.
    pub fn run(&self, id: &str) -> Option<&RolloutRun> {
        self.runs.iter().find(|r| r.id == id)
    }

    /// The artefact version `release` ships for `service`. A version with no release
    /// record (a channel seeded before any release existed) stands for itself.
    pub fn service_version(&self, release: &str, service: &str) -> String {
        self.releases
            .get(release)
            .and_then(|r| r.artefacts.get(service))
            .cloned()
            .unwrap_or_else(|| release.to_string())
    }

    /// What `channel`'s installations should run now, per service of `config`.
    pub fn desired(&self, channel: &str, config: &RingConfig) -> Option<Desired> {
        let state = self.channels.get(channel)?;
        let run = self.active_run(channel);
        let mut services = BTreeMap::new();
        for service in config.services() {
            services.insert(
                service.to_string(),
                self.service_version(&state.current, service),
            );
        }
        if let Some(run) = run {
            for ring in run.rings.iter().filter(|r| r.phase.on_target()) {
                for service in &ring.spec.services {
                    services.insert(service.clone(), self.service_version(&run.release, service));
                }
            }
        }
        Some(Desired {
            release: state.current.clone(),
            run: run.map(|r| r.id.clone()),
            target: run.map(|r| r.release.clone()),
            services,
        })
    }

    /// Start a run taking `channel` to `release`, its first ring at once. Refused if the
    /// channel or release is unknown, a run is active on the channel, or the release is
    /// already current.
    pub fn promote(
        &mut self,
        channel: &str,
        release: &str,
        config: &RingConfig,
        by: &str,
        now: u64,
    ) -> Result<(String, Vec<Event>), RolloutError> {
        config.validate().map_err(RolloutError::Invalid)?;
        let state = self
            .channels
            .get(channel)
            .ok_or_else(|| RolloutError::NotFound(format!("no channel {channel}")))?
            .clone();
        if !self.releases.contains_key(release) {
            return Err(RolloutError::NotFound(format!("no release {release}")));
        }
        if let Some(run) = self.active_run(channel) {
            return Err(RolloutError::Conflict(format!(
                "{} is already rolling {channel} out to {}; one run per channel at a time",
                run.id, run.release
            )));
        }
        if state.current == release {
            return Err(RolloutError::Conflict(format!(
                "{channel} is already on {release}"
            )));
        }
        let id = self.next_id();
        let mut rings: Vec<RingRun> = config.rings.iter().cloned().map(RingRun::new).collect();
        rings[0].start(now);
        let events = vec![
            Event::RunStarted {
                run: id.clone(),
                channel: channel.to_string(),
                from: state.current.clone(),
                release: release.to_string(),
            },
            Event::RingStarted {
                run: id.clone(),
                ring: rings[0].spec.name.clone(),
            },
        ];
        self.runs.push(RolloutRun {
            id: id.clone(),
            kind: RunKind::Promote,
            channel: channel.to_string(),
            release: release.to_string(),
            from: state.current,
            rings,
            report_ttl_secs: config.report_ttl_secs,
            state: RunState::Running,
            started_at: now,
            started_by: by.to_string(),
            ended_at: None,
            ended_by: None,
            reason: None,
        });
        self.trim();
        Ok((id, events))
    }

    /// Roll `channel` back, with a reason. With a run active, the run is rolled back:
    /// every service it changed is desired at the current release again (`to`, if
    /// given, must be that release). With none, the channel moves to `to`, or to its
    /// previous release, at once, recorded as a run of kind rollback.
    pub fn rollback(
        &mut self,
        channel: &str,
        to: Option<&str>,
        reason: &str,
        by: &str,
        now: u64,
    ) -> Result<(String, Vec<Event>), RolloutError> {
        if reason.trim().is_empty() {
            return Err(RolloutError::Invalid("a rollback needs a reason".into()));
        }
        let state = self
            .channels
            .get(channel)
            .ok_or_else(|| RolloutError::NotFound(format!("no channel {channel}")))?
            .clone();
        let reason = format!("manual: {reason}");
        if let Some(index) = self
            .runs
            .iter()
            .position(|r| r.channel == channel && r.state == RunState::Running)
        {
            if let Some(to) = to {
                if to != state.current {
                    return Err(RolloutError::Invalid(format!(
                        "{} is in progress: rolling it back returns {channel} to {}, not {to}",
                        self.runs[index].id, state.current
                    )));
                }
            }
            let run = &mut self.runs[index];
            let id = run.id.clone();
            let events = roll_back(run, None, &reason, by, now);
            return Ok((id, events));
        }
        let to = match to {
            Some(to) => to.to_string(),
            None => state.previous.clone().ok_or_else(|| {
                RolloutError::Conflict(format!(
                    "{channel} has no previous release to roll back to; name one"
                ))
            })?,
        };
        if to == state.current {
            return Err(RolloutError::Conflict(format!(
                "{channel} is already on {to}"
            )));
        }
        if !self.releases.contains_key(&to) && state.previous.as_deref() != Some(to.as_str()) {
            return Err(RolloutError::NotFound(format!("no release {to}")));
        }
        let id = self.next_id();
        self.runs.push(RolloutRun {
            id: id.clone(),
            kind: RunKind::Rollback,
            channel: channel.to_string(),
            release: to.clone(),
            from: state.current.clone(),
            rings: Vec::new(),
            report_ttl_secs: 0,
            state: RunState::RolledBack,
            started_at: now,
            started_by: by.to_string(),
            ended_at: Some(now),
            ended_by: Some(by.to_string()),
            reason: Some(reason.clone()),
        });
        let channel_state = self.channels.get_mut(channel).expect("checked above");
        channel_state.previous = Some(channel_state.current.clone());
        channel_state.current = to.clone();
        self.trim();
        Ok((
            id.clone(),
            vec![Event::RolledBack {
                run: id,
                channel: channel.to_string(),
                to,
                reason,
            }],
        ))
    }

    /// Step every active run at `now` with the latest `reports`: soak, pass, start the
    /// next ring, complete, or fail and roll back. Returns what happened; nothing
    /// happened if it is empty.
    pub fn tick(&mut self, now: u64, reports: &[InstallReport]) -> Vec<Event> {
        let mut events = Vec::new();
        for index in 0..self.runs.len() {
            if self.runs[index].state != RunState::Running {
                continue;
            }
            let run = &self.runs[index];
            let targets: BTreeMap<String, String> = run
                .rings
                .iter()
                .flat_map(|r| r.spec.services.iter())
                .map(|s| (s.clone(), self.service_version(&run.release, s)))
                .collect();
            let completed = step(&mut self.runs[index], &targets, now, reports, &mut events);
            if completed {
                let run = &self.runs[index];
                let (channel, release) = (run.channel.clone(), run.release.clone());
                if let Some(state) = self.channels.get_mut(&channel) {
                    state.previous = Some(std::mem::replace(&mut state.current, release));
                }
            }
        }
        self.trim();
        events
    }

    fn next_id(&mut self) -> String {
        self.next_run += 1;
        format!("run-{}", self.next_run)
    }

    /// Drop the oldest finished runs beyond [`MAX_FINISHED_RUNS`].
    fn trim(&mut self) {
        let finished = self
            .runs
            .iter()
            .filter(|r| r.state != RunState::Running)
            .count();
        let mut drop = finished.saturating_sub(MAX_FINISHED_RUNS);
        self.runs.retain(|r| {
            if drop > 0 && r.state != RunState::Running {
                drop -= 1;
                false
            } else {
                true
            }
        });
    }
}

/// Step one active run. Returns true when it completed.
fn step(
    run: &mut RolloutRun,
    targets: &BTreeMap<String, String>,
    now: u64,
    reports: &[InstallReport],
    events: &mut Vec<Event>,
) -> bool {
    let Some(index) = run.current_ring() else {
        // A running run with no ring in progress cannot happen; finish it safely.
        let events_now = roll_back(run, None, "no ring in progress", ORCHESTRATOR, now);
        events.extend(events_now);
        return false;
    };
    let ttl = run.report_ttl_secs;
    let participants: Vec<&InstallReport> = reports
        .iter()
        .filter(|r| r.channel == run.channel && now.saturating_sub(r.at) <= ttl)
        .collect();
    let ring = &mut run.rings[index];
    let mut not_ready: Vec<String> = Vec::new();
    let mut failure: Option<String> = None;
    for p in &participants {
        for service in &ring.spec.services {
            let target = &targets[service];
            let key = format!("{}/{service}", p.install_id);
            match p.services.get(service) {
                Some(s) if &s.version == target && s.healthy => {
                    ring.ready_seen.insert(key);
                }
                Some(s) if ring.ready_seen.contains(&key) => {
                    failure.get_or_insert_with(|| {
                        if &s.version == target {
                            format!(
                                "{service} on {} became unhealthy after reporting ready on {target}",
                                p.install_id
                            )
                        } else {
                            format!(
                                "{service} on {} went from {target} to {} after reporting ready",
                                p.install_id, s.version
                            )
                        }
                    });
                }
                Some(s) if &s.version == target => {
                    not_ready.push(format!("{service} on {} unhealthy", p.install_id))
                }
                Some(s) => not_ready.push(format!(
                    "{service} on {} at {}, not {target}",
                    p.install_id, s.version
                )),
                None => not_ready.push(format!("{service} on {} not reported", p.install_id)),
            }
        }
    }
    if participants.is_empty() {
        not_ready.push(format!("no installation on {} is reporting", run.channel));
    }
    let ring_name = ring.spec.name.clone();
    if let Some(reason) = failure {
        let reason = format!("ring {ring_name}: {reason}");
        events.extend(roll_back(run, Some(index), &reason, ORCHESTRATOR, now));
        return false;
    }
    if not_ready.is_empty() {
        let since = match ring.soak_since {
            Some(since) => since,
            None => {
                ring.soak_since = Some(now);
                ring.phase = RingPhase::Soaking;
                events.push(Event::RingReady {
                    run: run.id.clone(),
                    ring: ring_name.clone(),
                });
                now
            }
        };
        if now.saturating_sub(since) < ring.spec.soak_secs {
            return false;
        }
        ring.phase = RingPhase::Passed;
        ring.ended_at = Some(now);
        ring.deadline = None;
        events.push(Event::RingPassed {
            run: run.id.clone(),
            ring: ring_name,
        });
        match run.rings.get_mut(index + 1) {
            Some(next) => {
                next.start(now);
                events.push(Event::RingStarted {
                    run: run.id.clone(),
                    ring: next.spec.name.clone(),
                });
                false
            }
            None => {
                run.state = RunState::Completed;
                run.ended_at = Some(now);
                run.ended_by = Some(ORCHESTRATOR.to_string());
                events.push(Event::RunCompleted {
                    run: run.id.clone(),
                    channel: run.channel.clone(),
                    release: run.release.clone(),
                });
                true
            }
        }
    } else {
        if ring.phase == RingPhase::Soaking {
            // Readiness lost without a failure: a participant went quiet or a new one
            // joined. Wait again, with a fresh readiness window.
            ring.phase = RingPhase::Waiting;
            ring.soak_since = None;
            let fresh = now.saturating_add(ring.spec.ready_timeout_secs);
            ring.deadline = Some(ring.deadline.map_or(fresh, |d| d.max(fresh)));
        }
        if ring.deadline.is_some_and(|d| now >= d) {
            let reason = format!(
                "ring {ring_name}: not ready within {} s ({})",
                ring.spec.ready_timeout_secs,
                not_ready.join("; ")
            );
            events.extend(roll_back(run, Some(index), &reason, ORCHESTRATOR, now));
        }
        false
    }
}

/// Finish `run` as rolled back: the failed ring (if a gate failed) is marked failed,
/// every other ring that had started is marked rolled back.
fn roll_back(
    run: &mut RolloutRun,
    failed: Option<usize>,
    reason: &str,
    by: &str,
    now: u64,
) -> Vec<Event> {
    let mut events = Vec::new();
    for (i, ring) in run.rings.iter_mut().enumerate() {
        if Some(i) == failed {
            ring.phase = RingPhase::Failed;
            ring.reason = Some(reason.to_string());
            ring.ended_at = Some(now);
            ring.deadline = None;
            ring.soak_since = None;
            events.push(Event::RingFailed {
                run: run.id.clone(),
                ring: ring.spec.name.clone(),
                reason: reason.to_string(),
            });
        } else if ring.phase != RingPhase::Pending {
            ring.phase = RingPhase::RolledBack;
            ring.ended_at.get_or_insert(now);
            ring.deadline = None;
            ring.soak_since = None;
        }
    }
    run.state = RunState::RolledBack;
    run.ended_at = Some(now);
    run.ended_by = Some(by.to_string());
    run.reason = Some(reason.to_string());
    events.push(Event::RolledBack {
        run: run.id.clone(),
        channel: run.channel.clone(),
        to: run.from.clone(),
        reason: reason.to_string(),
    });
    events
}
