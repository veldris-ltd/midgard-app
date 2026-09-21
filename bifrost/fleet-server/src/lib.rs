//! # bifrost-fleet-server — the Bifrost fleet manager as a mesh service
//!
//! Agents on each installation check in over PQ-mTLS with the version they run and
//! their health; operators read the fleet: every installation's version and liveness,
//! the drift from each channel's desired version, and whether it has converged.
//! [FR-B-3] [RC-17] [NFR-SEC-1]
//!
//! Every request is authorised and audited through the governance spine before
//! anything is read or written — a check-in as action `CheckIn`, a read as
//! `ReadFleet` — and an agent may check in only for the installation its certificate
//! names. Accepted check-ins are journalled in a sealed [`DurableFleet`], so a restart
//! recovers them. [RC-12]
//!
//! Responses are one outcome byte ([`outcome`]) followed by UTF-8: a message, or for
//! [`FLEET_STATUS`] the fleet as JSON.

use std::sync::{Arc, Mutex, MutexGuard};

use audit_writer::LedgerStore;
use bifrost_fleet::{AgentReport, DurableFleet, NodeStatus};
use bifrost_rollout::VersionId;
use gateway::{RequestHandler, Router};
use governance::{GovContext, Governance};
use policy::{ClassificationLevel, Clearance, Purpose, Resource};
use pq_mtls::WorkloadIdentity;
use service_runtime::json::{parse, Json};
use service_runtime::strict_json::{no_repeated_keys, object, only_keys};
use telemetry::MetricsRegistry;

/// The method an agent checks in with; its payload is an encoded [`AgentReport`].
pub const CHECK_IN: &str = "CheckIn";
/// The method an operator reads the fleet with; its payload is empty.
pub const FLEET_STATUS: &str = "FleetStatus";
/// The policy action a check-in is authorised as.
pub const ACTION_CHECK_IN: &str = "CheckIn";
/// The policy action a fleet read is authorised as.
pub const ACTION_READ_FLEET: &str = "ReadFleet";

/// The first byte of every fleet response.
pub mod outcome {
    /// Done: recorded, or the fleet follows.
    pub const ACCEPTED: u8 = 0;
    /// Refused by policy, or an agent reporting for another installation.
    pub const DENIED: u8 = 1;
    /// The payload did not decode.
    pub const MALFORMED: u8 = 2;
    /// It could not be audited or recorded; nothing was changed.
    pub const FAILED: u8 = 3;
}

/// The fleet service: durable fleet state behind the governance spine.
pub struct FleetService<S: LedgerStore> {
    fleet: Mutex<DurableFleet>,
    governance: Mutex<Governance<S>>,
    resource: Resource,
    purpose: Purpose,
    stale_after: u64,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    metrics: Arc<MetricsRegistry>,
}

impl<S: LedgerStore + Send + 'static> FleetService<S> {
    /// Assemble the service. `resource` is what requests are authorised against and
    /// `purpose` the purpose they are audited under; a node that has not checked in for
    /// `stale_after` seconds of `clock` is reported stale.
    pub fn new(
        fleet: DurableFleet,
        governance: Governance<S>,
        resource: Resource,
        purpose: Purpose,
        stale_after: u64,
        clock: Arc<dyn Fn() -> u64 + Send + Sync>,
        metrics: Arc<MetricsRegistry>,
    ) -> Arc<Self> {
        metrics.describe(
            "fleet_requests_total",
            "Fleet requests, by method and outcome.",
        );
        Arc::new(Self {
            fleet: Mutex::new(fleet),
            governance: Mutex::new(governance),
            resource,
            purpose,
            stale_after,
            clock,
            metrics,
        })
    }

    /// The router the PQ-mTLS gateway serves: [`CHECK_IN`] and [`FLEET_STATUS`].
    pub fn router(self: &Arc<Self>) -> Router {
        let service = self.clone();
        let check_in: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.check_in(peer, body));
        let service = self.clone();
        let status: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, _: &[u8]| service.status(peer));
        Router::new()
            .route(CHECK_IN, check_in)
            .route(FLEET_STATUS, status)
    }

    /// Record an agent's check-in.
    pub fn check_in(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let report = AgentReport::decode(body);
        // A malformed check-in is still an authenticated call, and is still audited.
        let action = if report.is_ok() {
            ACTION_CHECK_IN
        } else {
            "CheckIn:MALFORMED"
        };
        let decision = match lock(&self.governance).authorize(
            &self.context(peer),
            &self.resource,
            action,
            now,
        ) {
            Ok(decision) => decision,
            Err(_) => {
                return self.reply(
                    CHECK_IN,
                    outcome::FAILED,
                    "the check-in could not be audited",
                )
            }
        };
        let report = match report {
            Ok(report) => report,
            Err(e) => return self.reply(CHECK_IN, outcome::MALFORMED, e),
        };
        if !decision.permitted {
            return self.reply(
                CHECK_IN,
                outcome::DENIED,
                "policy does not permit this identity to check in",
            );
        }
        if report.install_id != peer.id.0 {
            let recorded = lock(&self.governance).record_event(
                &peer.id.0,
                "CheckIn refused: reported for another installation",
                &format!("installation:{}", report.install_id),
                now,
            );
            let message = match recorded {
                Ok(_) => "an agent checks in only for its own installation",
                Err(_) => "an agent checks in only for its own installation (not audited)",
            };
            return self.reply(CHECK_IN, outcome::DENIED, message);
        }
        match lock(&self.fleet).check_in(report, now) {
            Ok(()) => self.reply(
                CHECK_IN,
                outcome::ACCEPTED,
                &format!("recorded at audit sequence {}", decision.audit_seq),
            ),
            Err(e) => self.reply(
                CHECK_IN,
                outcome::FAILED,
                &format!("the check-in was not recorded: {e}"),
            ),
        }
    }

    /// The fleet as JSON: channels, installations with liveness, drift, convergence.
    pub fn status(&self, peer: &WorkloadIdentity) -> Vec<u8> {
        let now = (self.clock)();
        let decision = lock(&self.governance).authorize(
            &self.context(peer),
            &self.resource,
            ACTION_READ_FLEET,
            now,
        );
        match decision {
            Ok(d) if d.permitted => {}
            Ok(_) => {
                return self.reply(
                    FLEET_STATUS,
                    outcome::DENIED,
                    "policy does not permit this identity to read the fleet",
                )
            }
            Err(_) => {
                return self.reply(
                    FLEET_STATUS,
                    outcome::FAILED,
                    "the read could not be audited",
                )
            }
        }

        let durable = lock(&self.fleet);
        let fleet = durable.fleet();
        let channels = fleet
            .channels()
            .map(|(name, desired)| {
                Json::obj(vec![
                    ("name", Json::s(name)),
                    ("desired", Json::s(&desired.0)),
                ])
            })
            .collect();
        let installations = fleet
            .installations()
            .map(|install| {
                let last_seen = fleet.checkin(&install.id).map_or(Json::Null, |c| {
                    Json::Int(i64::try_from(c.last_seen).unwrap_or(i64::MAX))
                });
                Json::obj(vec![
                    ("id", Json::s(&install.id)),
                    ("channel", Json::s(&install.channel)),
                    ("version", Json::s(&install.version.0)),
                    (
                        "status",
                        Json::s(status_name(fleet.node_status(
                            &install.id,
                            now,
                            self.stale_after,
                        ))),
                    ),
                    ("lastSeen", last_seen),
                ])
            })
            .collect();
        let drift = fleet
            .drift()
            .iter()
            .map(|d| {
                Json::obj(vec![
                    ("installId", Json::s(&d.install_id)),
                    ("channel", Json::s(&d.channel)),
                    ("from", Json::s(&d.from.0)),
                    ("to", Json::s(&d.to.0)),
                ])
            })
            .collect();
        let body = Json::obj(vec![
            ("channels", Json::Arr(channels)),
            ("installations", Json::Arr(installations)),
            ("drift", Json::Arr(drift)),
            ("converged", Json::Bool(fleet.is_converged())),
        ]);
        self.reply(FLEET_STATUS, outcome::ACCEPTED, &body.encode())
    }

    fn context(&self, peer: &WorkloadIdentity) -> GovContext {
        GovContext {
            subject: peer.id.0.clone(),
            roles: peer.roles.clone(),
            clearance: Clearance {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: Default::default(),
            },
            purpose: self.purpose.clone(),
        }
    }

    fn reply(&self, method: &str, code: u8, message: &str) -> Vec<u8> {
        let label = match code {
            outcome::ACCEPTED => "accepted",
            outcome::DENIED => "denied",
            outcome::MALFORMED => "malformed",
            _ => "failed",
        };
        self.metrics.inc(
            "fleet_requests_total",
            &[("method", method), ("outcome", label)],
        );
        let mut out = vec![code];
        out.extend_from_slice(message.as_bytes());
        out
    }
}

/// A lock that stays usable after a panic elsewhere: every write is journalled before
/// memory changes, so the state behind a poisoned lock is still the journalled state.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn status_name(status: NodeStatus) -> &'static str {
    match status {
        NodeStatus::Live => "live",
        NodeStatus::Unhealthy => "unhealthy",
        NodeStatus::Stale => "stale",
        NodeStatus::Absent => "absent",
    }
}

/// Parse a channels file — each release channel's desired version — strictly:
///
/// ```json
/// { "version": 1, "channels": { "stable": "1.5.0", "edge": "1.6.0-rc.1" } }
/// ```
///
/// Unknown or repeated keys, another version, a channel name other than letters,
/// digits, `_`, `-` and `.`, and a blank or non-string version all refuse the file.
pub fn parse_channels(text: &str) -> Result<Vec<(String, VersionId)>, String> {
    let doc = parse(text).map_err(|e| format!("not JSON: {e}"))?;
    only_keys(
        object(&doc, "the file")?,
        "the file",
        &["version", "channels"],
    )?;
    match doc.get("version") {
        Some(Json::Int(1)) => {}
        Some(other) => return Err(format!("unsupported version {}", other.encode())),
        None => return Err("missing \"version\"".into()),
    }
    let channels = doc
        .get("channels")
        .ok_or_else(|| "missing \"channels\"".to_string())?;
    let entries = object(channels, "channels")?;
    no_repeated_keys(entries, "channels")?;
    entries
        .iter()
        .map(|(name, desired)| {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            {
                return Err(format!(
                    "channel name \"{name}\" may contain only letters, digits, _, - and ."
                ));
            }
            match desired {
                Json::Str(v) if !v.trim().is_empty() => Ok((name.clone(), VersionId::new(v))),
                other => Err(format!(
                    "channels.{name} must be a version string, not {}",
                    other.encode()
                )),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channels_file_is_read_strictly() {
        let channels =
            parse_channels(r#"{"version":1,"channels":{"stable":"1.5.0","edge":"1.6.0-rc.1"}}"#)
                .unwrap();
        assert_eq!(
            channels,
            [
                ("stable".to_string(), VersionId::new("1.5.0")),
                ("edge".to_string(), VersionId::new("1.6.0-rc.1")),
            ]
        );
        assert!(parse_channels(r#"{"version":1,"channels":{}}"#)
            .unwrap()
            .is_empty());

        for bad in [
            r#"{"version":2,"channels":{}}"#,
            r#"{"channels":{}}"#,
            r#"{"version":1}"#,
            r#"{"version":1,"channels":{},"extra":true}"#,
            r#"{"version":1,"channels":{"stable":"1.5.0","stable":"1.4.0"}}"#,
            r#"{"version":1,"channels":{"stable":""}}"#,
            r#"{"version":1,"channels":{"stable":150}}"#,
            r#"{"version":1,"channels":{"sta ble":"1.5.0"}}"#,
            r#"{"version":1,"channels":[]}"#,
            "not json",
        ] {
            assert!(parse_channels(bad).is_err(), "{bad}");
        }
    }
}
