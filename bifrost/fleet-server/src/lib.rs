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
//! It also holds the artefact registry ([`registry`]): signed, SBOM-attested service
//! builds, uploaded in chunks ([`REGISTER_ARTEFACT`], [`PUT_ARTEFACT_CHUNK`],
//! [`COMMIT_ARTEFACT`], authorised as `RegisterArtefact`), verified by the
//! `bifrost-artefact` gate at the commit, re-checked whenever they are read, and sealed
//! at rest. Reads are three actions: listing ([`LIST_ARTEFACTS`], `ReadArtefacts`),
//! downloading ([`GET_ARTEFACT`], `DownloadArtefact`, for agents) and re-verifying
//! ([`VERIFY_ARTEFACT`], `VerifyArtefact`). A download re-checks the seal of the stored
//! copy and runs the whole gate only on sealed bytes it has not passed before;
//! `VerifyArtefact` always runs the whole gate (see [`registry`]'s reads). A refusal is recorded with its reason, audited,
//! and listed as rejected. [FR-B-1] [FR-B-4] [NFR-PQ-3]
//!
//! It also runs releases out ([`releases`], built on `bifrost_rollout::runs`): a release
//! manager creates a release from verified artefacts ([`CREATE_RELEASE`]), promotes it
//! to a channel ([`PROMOTE`]) and can roll it back ([`ROLLBACK`]); operators and release
//! managers read releases and runs ([`LIST_RELEASES`], [`LIST_ROLLOUTS`],
//! [`GET_ROLLOUT`]). A promotion raises the desired version ring by ring; agents report
//! per-service versions and health with [`CHECK_IN_SERVICES`] and get their desired
//! versions back; [`FleetService::tick`], on a timer, gates each ring and rolls back on a
//! failure. The state is sealed at rest and a run resumes after a restart.
//!
//! Responses are one outcome byte ([`outcome`]) followed by UTF-8: a message, or for
//! [`FLEET_STATUS`] and an accepted registry call, JSON. Registry requests are JSON too,
//! so a browser bridge can pass them through; artefact bytes travel as base64.

pub mod b64;
pub mod client;
pub mod registry;
pub mod releases;
pub mod server;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use audit_writer::LedgerStore;
use bifrost_artefact::SignedArtefact;
use bifrost_artefact::{ArtefactManifest, BIFROST_SIG_ALG};
use bifrost_fleet::{AgentReport, DurableFleet, NodeStatus};
use bifrost_rollout::runs::{
    check_name, Event, InstallReport, Release, RingConfig, RolloutError, RolloutRun, Rollouts,
    ServiceStatus, ORCHESTRATOR,
};
use bifrost_rollout::VersionId;
use gateway::{RequestHandler, Router};
use governance::{GovContext, Governance};
use policy::{ClassificationLevel, Clearance, Purpose, Resource};
use pq_mtls::WorkloadIdentity;
use pqc_agility::Signature;
use registry::{
    Fetch, Listed, NewUpload, Recheck, Record, Refusal, Registry, Rejection, StoreError,
    CHUNK_BYTES, LIST_PAGE, MAX_LIST_PAGE,
};
use releases::{release_json, rings_json, run_json, ReleaseStore};
use service_runtime::json::{parse, Json};
use service_runtime::strict_json::{no_repeated_keys, object, only_keys, text};
use telemetry::MetricsRegistry;

/// The method an agent checks in with; its payload is an encoded [`AgentReport`].
pub const CHECK_IN: &str = "CheckIn";
/// The method an operator reads the fleet with; its payload is empty.
pub const FLEET_STATUS: &str = "FleetStatus";
/// The policy action a check-in is authorised as.
pub const ACTION_CHECK_IN: &str = "CheckIn";
/// The policy action a fleet read is authorised as.
pub const ACTION_READ_FLEET: &str = "ReadFleet";

/// Begin an artefact upload: `{"manifest": {"id", "version", "component",
/// "payloadSha256", "sbomSha256"}, "sbom" (base64, at most 512 KiB), "signature" (hex),
/// "signer" (key id), "size"}`. Answers `{"uploadId", "chunkBytes"}`.
pub const REGISTER_ARTEFACT: &str = "RegisterArtefact";
/// Send the next slice of an upload: `{"uploadId", "offset", "data" (base64)}`.
/// Answers `{"received"}`.
pub const PUT_ARTEFACT_CHUNK: &str = "PutArtefactChunk";
/// Finish an upload: `{"uploadId"}`. The gate runs here; answers the artefact as listed.
pub const COMMIT_ARTEFACT: &str = "CommitArtefact";
/// List verified, unverifiable and rejected artefacts, a page at a time:
/// `{"cursor"?, "limit"?}`. Answers `{"signers", "artefacts", "next"}`; `next` is the
/// cursor of the following page, or null.
pub const LIST_ARTEFACTS: &str = "ListArtefacts";
/// Read a verified artefact, re-verified: `{"id", "version", "part"?, "offset"?,
/// "length"?}`, where `part` is `"payload"` (the default) or `"sbom"`. Answers the
/// manifest, signer, signature, the part's size and one slice of it, as base64.
pub const GET_ARTEFACT: &str = "GetArtefact";
/// Re-verify an artefact at rest now: `{"id", "version"}`.
pub const VERIFY_ARTEFACT: &str = "VerifyArtefact";
/// The policy action every step of an upload is authorised as.
pub const ACTION_REGISTER_ARTEFACT: &str = "RegisterArtefact";
/// The policy action listing the registry is authorised as.
pub const ACTION_READ_ARTEFACTS: &str = "ReadArtefacts";
/// The policy action downloading an artefact ([`GET_ARTEFACT`]) is authorised as.
pub const ACTION_DOWNLOAD_ARTEFACT: &str = "DownloadArtefact";
/// The policy action re-verifying an artefact ([`VERIFY_ARTEFACT`]) is authorised as.
pub const ACTION_VERIFY_ARTEFACT: &str = "VerifyArtefact";

/// Check in with per-service status (JSON), and get each service's desired version back:
/// `{"installId", "channel", "services": {name: {"version", "healthy"}}}`. Answers
/// `{"installId", "channel", "release", "run", "target", "desired": {name: version}}`
/// (`desired` null for a channel the orchestrator does not know). Authorised as
/// [`ACTION_CHECK_IN`], like [`CHECK_IN`], which stays as it was for simple agents.
pub const CHECK_IN_SERVICES: &str = "CheckInServices";
/// Create a release: `{"version", "artefacts"?: {service: version}}`. Without
/// `artefacts`, every ring service at `version`. Every artefact must be held and pass
/// the whole verification gate now. Answers the release.
pub const CREATE_RELEASE: &str = "CreateRelease";
/// Releases, channels (with their desired versions) and the rings: `{}`.
pub const LIST_RELEASES: &str = "ListReleases";
/// Start a rollout of a release to a channel: `{"channel", "release"}`. Answers the run.
pub const PROMOTE: &str = "Promote";
/// Roll a channel back: `{"channel", "reason", "to"?}`. Answers the run.
pub const ROLLBACK: &str = "Rollback";
/// Runs, newest first: `{"channel"?, "limit"?}`. Answers `{"rollouts"}`.
pub const LIST_ROLLOUTS: &str = "ListRollouts";
/// One run: `{"id"}`.
pub const GET_ROLLOUT: &str = "GetRollout";
/// The policy action creating a release is authorised as.
pub const ACTION_CREATE_RELEASE: &str = "CreateRelease";
/// The policy action listing releases is authorised as.
pub const ACTION_LIST_RELEASES: &str = "ListReleases";
/// The policy action promoting is authorised as.
pub const ACTION_PROMOTE: &str = "Promote";
/// The policy action rolling back is authorised as.
pub const ACTION_ROLLBACK: &str = "Rollback";
/// The policy action listing runs is authorised as.
pub const ACTION_LIST_ROLLOUTS: &str = "ListRollouts";
/// The policy action reading one run is authorised as.
pub const ACTION_GET_ROLLOUT: &str = "GetRollout";

/// Installations whose per-service reports are held in memory at most.
const MAX_REPORTS: usize = 1024;
/// Services one per-service check-in may report at most.
const MAX_REPORTED_SERVICES: usize = 64;
/// Runs one [`LIST_ROLLOUTS`] answers at most.
const MAX_ROLLOUTS_PAGE: usize = 200;

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
    /// No such artefact or upload.
    pub const NOT_FOUND: u8 = 4;
    /// The gate refused the artefact (recorded and listed as rejected), or a held
    /// artefact no longer verifies and is not served.
    pub const REJECTED: u8 = 5;
    /// That artefact id and version are already registered.
    pub const CONFLICT: u8 = 6;
}

/// The fleet service: durable fleet state behind the governance spine.
pub struct FleetService<S: LedgerStore> {
    fleet: Mutex<DurableFleet>,
    registry: Mutex<Registry>,
    releases: Mutex<ReleaseStore>,
    reports: Mutex<BTreeMap<String, InstallReport>>,
    rings: RingConfig,
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
    /// `stale_after` seconds of `clock` is reported stale. `releases` is the
    /// orchestrator's state and `rings` the rings new runs use.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fleet: DurableFleet,
        registry: Registry,
        releases: ReleaseStore,
        rings: RingConfig,
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
            registry: Mutex::new(registry),
            releases: Mutex::new(releases),
            reports: Mutex::new(BTreeMap::new()),
            rings,
            governance: Mutex::new(governance),
            resource,
            purpose,
            stale_after,
            clock,
            metrics,
        })
    }

    /// The router the PQ-mTLS gateway serves: [`CHECK_IN`], [`FLEET_STATUS`] and the
    /// registry's methods.
    pub fn router(self: &Arc<Self>) -> Router {
        let service = self.clone();
        let check_in: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.check_in(peer, body));
        let service = self.clone();
        let status: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, _: &[u8]| service.status(peer));
        let mut router = Router::new()
            .route(CHECK_IN, check_in)
            .route(FLEET_STATUS, status);
        type Handler<S> = fn(&FleetService<S>, &WorkloadIdentity, &Json, u64) -> (u8, String);
        let routes: [(&'static str, &'static str, Handler<S>); 13] = [
            (
                REGISTER_ARTEFACT,
                ACTION_REGISTER_ARTEFACT,
                Self::register_artefact,
            ),
            (
                PUT_ARTEFACT_CHUNK,
                ACTION_REGISTER_ARTEFACT,
                Self::put_artefact_chunk,
            ),
            (
                COMMIT_ARTEFACT,
                ACTION_REGISTER_ARTEFACT,
                Self::commit_artefact,
            ),
            (LIST_ARTEFACTS, ACTION_READ_ARTEFACTS, Self::list_artefacts),
            (GET_ARTEFACT, ACTION_DOWNLOAD_ARTEFACT, Self::get_artefact),
            (
                VERIFY_ARTEFACT,
                ACTION_VERIFY_ARTEFACT,
                Self::verify_artefact,
            ),
            (CHECK_IN_SERVICES, ACTION_CHECK_IN, Self::check_in_services),
            (CREATE_RELEASE, ACTION_CREATE_RELEASE, Self::create_release),
            (LIST_RELEASES, ACTION_LIST_RELEASES, Self::list_releases),
            (PROMOTE, ACTION_PROMOTE, Self::promote),
            (ROLLBACK, ACTION_ROLLBACK, Self::rollback),
            (LIST_ROLLOUTS, ACTION_LIST_ROLLOUTS, Self::list_rollouts),
            (GET_ROLLOUT, ACTION_GET_ROLLOUT, Self::get_rollout),
        ];
        for (method, action, handler) in routes {
            let service = self.clone();
            let route: Arc<dyn RequestHandler> =
                Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| {
                    service.json_call(peer, body, method, action, handler)
                });
            router = router.route(method, route);
        }
        router
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

    /// The spine every JSON call (the registry's, the orchestrator's, the per-service
    /// check-in) goes through: the body must be a JSON object (an empty body is `{}`);
    /// the call is authorised as `action` — or `action:MALFORMED` for a body that is not
    /// one, still audited — and only then handled.
    fn json_call(
        &self,
        peer: &WorkloadIdentity,
        body: &[u8],
        method: &str,
        action: &str,
        handler: fn(&Self, &WorkloadIdentity, &Json, u64) -> (u8, String),
    ) -> Vec<u8> {
        let now = (self.clock)();
        let request = match std::str::from_utf8(body) {
            Ok(t) if t.trim().is_empty() => Ok(Json::Obj(Vec::new())),
            Ok(t) => parse(t).and_then(|doc| match doc {
                Json::Obj(_) => Ok(doc),
                _ => Err("the request must be a JSON object".to_string()),
            }),
            Err(_) => Err("the request is not UTF-8".to_string()),
        };
        let audited = if request.is_ok() {
            action.to_string()
        } else {
            format!("{action}:MALFORMED")
        };
        let decision = match lock(&self.governance).authorize(
            &self.context(peer),
            &self.resource,
            &audited,
            now,
        ) {
            Ok(decision) => decision,
            Err(_) => {
                return self.reply(method, outcome::FAILED, "the request could not be audited")
            }
        };
        let request = match request {
            Ok(request) => request,
            Err(e) => return self.reply(method, outcome::MALFORMED, &e),
        };
        if !decision.permitted {
            return self.reply(
                method,
                outcome::DENIED,
                &format!("policy does not permit this identity to {action}"),
            );
        }
        let (code, message) = handler(self, peer, &request, now);
        self.reply(method, code, &message)
    }

    fn register_artefact(&self, peer: &WorkloadIdentity, req: &Json, now: u64) -> (u8, String) {
        let new = match parse_new_upload(req) {
            Ok(new) => new,
            Err(e) => return (outcome::MALFORMED, e),
        };
        let mut id = [0u8; 16];
        if pqc_agility::OsRng.try_fill(&mut id).is_err() {
            return (outcome::FAILED, "no randomness for an upload id".into());
        }
        let upload_id = attest::hex::hex(&id);
        let begun = lock(&self.registry).begin(&peer.id.0, upload_id.clone(), new, now);
        match begun {
            Ok(()) => (
                outcome::ACCEPTED,
                Json::obj(vec![
                    ("uploadId", Json::s(&upload_id)),
                    ("chunkBytes", Json::Int(CHUNK_BYTES as i64)),
                ])
                .encode(),
            ),
            Err((StoreError::Refused(reason), Some(upload))) => {
                self.refuse(peer, &upload, reason, now)
            }
            Err((e, _)) => store_error(e),
        }
    }

    fn put_artefact_chunk(&self, peer: &WorkloadIdentity, req: &Json, now: u64) -> (u8, String) {
        let parsed = (|| -> Result<(String, u64, Vec<u8>), String> {
            only_keys(
                object(req, "the chunk")?,
                "the chunk",
                &["uploadId", "offset", "data"],
            )?;
            let upload_id = text(req, "the chunk", "uploadId")?.to_string();
            let offset = count(req, "the chunk", "offset")?;
            let data = match req.get("data") {
                Some(Json::Str(s)) => {
                    b64::decode(s).map_err(|e| format!("the chunk: data: {e}"))?
                }
                _ => return Err("the chunk: \"data\" must be a base64 string".into()),
            };
            Ok((upload_id, offset, data))
        })();
        let (upload_id, offset, data) = match parsed {
            Ok(p) => p,
            Err(e) => return (outcome::MALFORMED, e),
        };
        let received = lock(&self.registry).chunk(&peer.id.0, &upload_id, offset, &data, now);
        match received {
            Ok(n) => (
                outcome::ACCEPTED,
                Json::obj(vec![("received", Json::Int(n as i64))]).encode(),
            ),
            Err(e) => store_error(e),
        }
    }

    fn commit_artefact(&self, peer: &WorkloadIdentity, req: &Json, now: u64) -> (u8, String) {
        let upload_id = match only_keys(
            object(req, "the commit").unwrap_or(&[]),
            "the commit",
            &["uploadId"],
        )
        .and_then(|_| text(req, "the commit", "uploadId"))
        {
            Ok(id) => id.to_string(),
            Err(e) => return (outcome::MALFORMED, e),
        };
        let mut registry = lock(&self.registry);
        let upload = match registry.take(&peer.id.0, &upload_id, now) {
            Ok(upload) => upload,
            Err(e) => return store_error(e),
        };
        if let Err(reason) = registry.gate(&upload) {
            drop(registry);
            return self.refuse(peer, &upload, reason, now);
        }
        let (id, version) = (
            upload.manifest().id.clone(),
            upload.manifest().version.clone(),
        );
        let name = format!("artefact:{id}@{version}");
        // Stored first, then audited: the ledger never says "verified" for something
        // that is not held, and what is held is withdrawn if the ledger cannot say so.
        let record = match registry.admit(upload, &peer.id.0, now) {
            Ok(record) => record,
            Err(e) => {
                let _ = lock(&self.governance).record_event(
                    &peer.id.0,
                    "RegisterArtefact failed: not stored",
                    &name,
                    now,
                );
                return store_error(e);
            }
        };
        if lock(&self.governance)
            .record_event(&peer.id.0, "RegisterArtefact verified", &name, now)
            .is_err()
        {
            let withdrawn = registry.withdraw(&id, &version);
            return (
                outcome::FAILED,
                match withdrawn {
                    Ok(()) => "the registration could not be audited; nothing was stored".into(),
                    Err(e) => format!(
                        "the registration could not be audited, and withdrawing it failed: {e}"
                    ),
                },
            );
        }
        (outcome::ACCEPTED, record_json(&record).encode())
    }

    /// Record a refusal in the registry and the audit ledger, and answer it.
    fn refuse(
        &self,
        peer: &WorkloadIdentity,
        upload: &registry::Upload,
        reason: Refusal,
        now: u64,
    ) -> (u8, String) {
        let recorded = lock(&self.registry).reject(Rejection::of(upload, reason, &peer.id.0, now));
        let audited = lock(&self.governance).record_event(
            &peer.id.0,
            &format!("RegisterArtefact refused: {}", reason.code()),
            &format!(
                "artefact:{}@{}",
                upload.manifest().id,
                upload.manifest().version
            ),
            now,
        );
        let mut message = StoreError::Refused(reason).to_string();
        match recorded {
            Ok(true) => {}
            Ok(false) => message.push_str(" (not listed: too many refusals)"),
            Err(_) => message.push_str(" (the refusal could not be recorded)"),
        }
        if audited.is_err() {
            message.push_str(" (not audited)");
        }
        (outcome::REJECTED, message)
    }

    fn list_artefacts(&self, _: &WorkloadIdentity, req: &Json, _: u64) -> (u8, String) {
        let parsed = (|| -> Result<(Option<String>, usize), String> {
            only_keys(
                object(req, "the request")?,
                "the request",
                &["cursor", "limit"],
            )?;
            let cursor = match req.get("cursor") {
                None | Some(Json::Null) => None,
                Some(_) => Some(text(req, "the request", "cursor")?.to_string()),
            };
            let limit = match req.get("limit") {
                None => LIST_PAGE,
                Some(_) => {
                    usize::try_from(count(req, "the request", "limit")?).unwrap_or(MAX_LIST_PAGE)
                }
            };
            Ok((cursor, limit))
        })();
        let (cursor, limit) = match parsed {
            Ok(p) => p,
            Err(e) => return (outcome::MALFORMED, e),
        };
        let registry = lock(&self.registry);
        let (page, next) = match registry.page(cursor.as_deref(), limit) {
            Ok(p) => p,
            Err(e) => return store_error(e),
        };
        let items = page
            .into_iter()
            .map(|item| match item {
                Listed::Held(r) => record_json(r),
                Listed::Rejected(r) => Json::obj(vec![
                    ("id", Json::s(&r.id)),
                    ("version", Json::s(&r.version)),
                    ("component", Json::s(&r.component)),
                    ("status", Json::s("rejected")),
                    ("reason", Json::s(r.reason.code())),
                    ("detail", Json::s(r.reason.detail())),
                    ("signer", Json::s(&r.signer)),
                    ("payloadSha256", Json::s(&r.payload_sha256)),
                    ("submittedBy", Json::s(&r.by)),
                    (
                        "submittedAt",
                        Json::Int(i64::try_from(r.at).unwrap_or(i64::MAX)),
                    ),
                ]),
            })
            .collect();
        let signers = registry.signer_ids();
        let body = Json::obj(vec![
            (
                "signers",
                Json::Arr(signers.iter().map(|s| Json::s(s)).collect()),
            ),
            ("artefacts", Json::Arr(items)),
            ("next", next.map_or(Json::Null, |c| Json::s(&c))),
        ]);
        (outcome::ACCEPTED, body.encode())
    }

    fn get_artefact(&self, peer: &WorkloadIdentity, req: &Json, now: u64) -> (u8, String) {
        let parsed = (|| -> Result<(String, String, bool, u64, u64), String> {
            only_keys(
                object(req, "the request")?,
                "the request",
                &["id", "version", "part", "offset", "length"],
            )?;
            let sbom = match req.get("part") {
                None => false,
                Some(Json::Str(p)) if p == "payload" => false,
                Some(Json::Str(p)) if p == "sbom" => true,
                Some(_) => return Err("the request: \"part\" is \"payload\" or \"sbom\"".into()),
            };
            let offset = match req.get("offset") {
                None => 0,
                Some(_) => count(req, "the request", "offset")?,
            };
            let length = match req.get("length") {
                None => CHUNK_BYTES as u64,
                Some(_) => count(req, "the request", "length")?,
            };
            Ok((
                text(req, "the request", "id")?.to_string(),
                text(req, "the request", "version")?.to_string(),
                sbom,
                offset,
                length.min(CHUNK_BYTES as u64),
            ))
        })();
        let (id, version, sbom, offset, length) = match parsed {
            Ok(p) => p,
            Err(e) => return (outcome::MALFORMED, e),
        };
        // Every part's first slice re-checks the sealed copy; its later slices come from
        // the copy that check passed.
        let recheck = if offset == 0 {
            Recheck::Seal
        } else {
            Recheck::Cached
        };
        let fetched =
            self.fetch_verified(peer, ACTION_DOWNLOAD_ARTEFACT, &id, &version, recheck, now);
        let (record, artefact) = match fetched {
            Ok(f) => f,
            Err(answer) => return answer,
        };
        let bytes: &[u8] = if sbom {
            artefact.sbom.as_bytes()
        } else {
            &artefact.payload
        };
        let size = bytes.len() as u64;
        if offset > size {
            return (
                outcome::MALFORMED,
                format!("offset {offset} is past the end ({size} bytes)"),
            );
        }
        // At most CHUNK_BYTES as base64, beside identifiers and hex: inside one frame
        // (see the registry's frame budget).
        let end = (offset + length).min(size);
        let slice = &bytes[offset as usize..end as usize];
        let fields = vec![
            ("manifest", manifest_json(&record.manifest)),
            ("signer", Json::s(&record.signer)),
            ("alg", Json::s(artefact.signature.alg().as_str())),
            (
                "signature",
                Json::s(&attest::hex::hex(artefact.signature.as_bytes())),
            ),
            ("part", Json::s(if sbom { "sbom" } else { "payload" })),
            ("size", Json::Int(size as i64)),
            ("offset", Json::Int(offset as i64)),
            ("data", Json::s(&b64::encode(slice))),
        ];
        (outcome::ACCEPTED, Json::obj(fields).encode())
    }

    fn verify_artefact(&self, peer: &WorkloadIdentity, req: &Json, now: u64) -> (u8, String) {
        let parsed = only_keys(
            object(req, "the request").unwrap_or(&[]),
            "the request",
            &["id", "version"],
        )
        .and_then(|_| {
            Ok((
                text(req, "the request", "id")?.to_string(),
                text(req, "the request", "version")?.to_string(),
            ))
        });
        let (id, version) = match parsed {
            Ok(p) => p,
            Err(e) => return (outcome::MALFORMED, e),
        };
        match self.fetch_verified(
            peer,
            ACTION_VERIFY_ARTEFACT,
            &id,
            &version,
            Recheck::Full,
            now,
        ) {
            Ok((record, _)) => (outcome::ACCEPTED, record_json(&record).encode()),
            Err(answer) => answer,
        }
    }

    /// Fetch and check an artefact for `peer`, as `recheck` asks. The sealed copy is
    /// read and checked outside the registry's lock. A verification that fails now
    /// marks the artefact unverifiable for good and is audited under `action`, with the
    /// reason.
    fn fetch_verified(
        &self,
        peer: &WorkloadIdentity,
        action: &str,
        id: &str,
        version: &str,
        recheck: Recheck,
        now: u64,
    ) -> Result<(Record, Arc<SignedArtefact>), (u8, String)> {
        let begun = lock(&self.registry).begin_fetch(id, version, recheck);
        let fetched = begun.and_then(|step| match step {
            Fetch::Ready(record, artefact) => Ok((record, artefact)),
            Fetch::Read(read) => {
                let outcome = read.run();
                lock(&self.registry).finish_fetch(read, outcome)
            }
        });
        match fetched {
            Ok(f) => Ok(f),
            Err(StoreError::VerifyFailed(reason)) => {
                let audited = lock(&self.governance).record_event(
                    &peer.id.0,
                    &format!("{action} verify failed: {reason}"),
                    &format!("artefact:{id}@{version}"),
                    now,
                );
                let mut message = format!("{reason}; it will not be served");
                if audited.is_err() {
                    message.push_str(" (not audited)");
                }
                Err((outcome::REJECTED, message))
            }
            Err(e) => Err(store_error(e)),
        }
    }

    // --- the orchestrator -------------------------------------------------------------

    /// Step every active rollout with the latest per-service reports: soak, pass, start
    /// the next ring, complete, or fail and roll back. The server calls it on a timer.
    /// Changes are sealed before they take effect, then audited as the orchestrator;
    /// what happened is returned (for the log), with a problem if the change could not
    /// be saved (then nothing changed, and the next tick tries again) or audited.
    pub fn tick(&self) -> (Vec<Event>, Option<String>) {
        let now = (self.clock)();
        let reports: Vec<InstallReport> = lock(&self.reports).values().cloned().collect();
        let mut store = lock(&self.releases);
        let mut next = store.state().clone();
        let events = next.tick(now, &reports);
        if next == *store.state() {
            return (events, None);
        }
        if let Err(e) = store.commit(next) {
            return (Vec::new(), Some(format!("not saved, nothing changed: {e}")));
        }
        let channels = channel_currents(store.state());
        drop(store);
        self.sync_channels(&channels);
        let problem = (!self.audit_events(ORCHESTRATOR, &events, now))
            .then(|| "not every rollout event could be audited".to_string());
        (events, problem)
    }

    /// Make the fleet's channel desires (what FleetStatus's drift compares against) the
    /// orchestrator's current releases.
    fn sync_channels(&self, channels: &[(String, String)]) {
        let mut fleet = lock(&self.fleet);
        for (name, current) in channels {
            let desired = VersionId::new(current);
            if fleet.fleet().channel_desired(name) != Some(&desired) {
                let _ = fleet.set_channel(name.clone(), desired);
            }
        }
    }

    fn audit_events(&self, actor: &str, events: &[Event], now: u64) -> bool {
        let mut governance = lock(&self.governance);
        events.iter().all(|e| {
            governance
                .record_event(actor, &e.action(), &format!("rollout:{}", e.run()), now)
                .is_ok()
        })
    }

    /// Change the orchestrator's state with `change`, sealed before it takes effect, and
    /// audit what happened as `peer`.
    fn change<T>(
        &self,
        peer: &WorkloadIdentity,
        now: u64,
        change: impl FnOnce(&mut Rollouts) -> Result<(T, Vec<Event>), RolloutError>,
    ) -> Result<(T, Rollouts, bool), (u8, String)> {
        let mut store = lock(&self.releases);
        let mut next = store.state().clone();
        let (value, events) = change(&mut next).map_err(rollout_error)?;
        store
            .commit(next)
            .map_err(|e| (outcome::FAILED, format!("not saved, nothing changed: {e}")))?;
        let state = store.state().clone();
        drop(store);
        self.sync_channels(&channel_currents(&state));
        let audited = self.audit_events(&peer.id.0, &events, now);
        Ok((value, state, audited))
    }

    fn check_in_services(&self, peer: &WorkloadIdentity, req: &Json, now: u64) -> (u8, String) {
        let parsed = (|| -> Result<InstallReport, String> {
            let at = "the check-in";
            only_keys(object(req, at)?, at, &["installId", "channel", "services"])?;
            let install_id = text(req, at, "installId")?.to_string();
            let channel = text(req, at, "channel")?.to_string();
            check_name(&channel, "a channel name")?;
            let services = object(
                req.get("services")
                    .ok_or("the check-in: missing \"services\"")?,
                "services",
            )?;
            no_repeated_keys(services, "services")?;
            if services.len() > MAX_REPORTED_SERVICES {
                return Err(format!(
                    "the check-in reports more than {MAX_REPORTED_SERVICES} services"
                ));
            }
            let mut map = BTreeMap::new();
            for (name, status) in services {
                check_name(name, "a service name")?;
                let sat = format!("services.{name}");
                only_keys(object(status, &sat)?, &sat, &["version", "healthy"])?;
                let version = text(status, &sat, "version")?.to_string();
                check_name(&version, "a version")?;
                let healthy = match status.get("healthy") {
                    Some(Json::Bool(b)) => *b,
                    _ => return Err(format!("{sat}: \"healthy\" must be true or false")),
                };
                map.insert(name.clone(), ServiceStatus { version, healthy });
            }
            Ok(InstallReport {
                install_id,
                channel,
                at: now,
                services: map,
            })
        })();
        let report = match parsed {
            Ok(r) => r,
            Err(e) => return (outcome::MALFORMED, e),
        };
        if report.install_id != peer.id.0 {
            let recorded = lock(&self.governance).record_event(
                &peer.id.0,
                "CheckIn refused: reported for another installation",
                &format!("installation:{}", report.install_id),
                now,
            );
            let mut message = "an agent checks in only for its own installation".to_string();
            if recorded.is_err() {
                message.push_str(" (not audited)");
            }
            return (outcome::DENIED, message);
        }
        // The fleet view keeps one version per installation: the services' common
        // version, or "mixed" while they differ.
        let mut versions = report.services.values().map(|s| s.version.as_str());
        let first = versions.next().unwrap_or("none");
        let summary = if versions.all(|v| v == first) {
            first
        } else {
            "mixed"
        };
        let legacy = AgentReport::new(
            report.install_id.clone(),
            report.channel.clone(),
            VersionId::new(summary),
            report.services.values().all(|s| s.healthy),
        );
        if let Err(e) = lock(&self.fleet).check_in(legacy, now) {
            return (
                outcome::FAILED,
                format!("the check-in was not recorded: {e}"),
            );
        }
        let (install_id, channel) = (report.install_id.clone(), report.channel.clone());
        {
            let mut reports = lock(&self.reports);
            let stale_after = self.stale_after;
            reports.retain(|_, r| now.saturating_sub(r.at) <= stale_after);
            if reports.len() < MAX_REPORTS || reports.contains_key(&install_id) {
                reports.insert(install_id.clone(), report);
            }
        }
        let desired = lock(&self.releases).state().desired(&channel, &self.rings);
        let body = match desired {
            Some(d) => Json::obj(vec![
                ("installId", Json::s(&install_id)),
                ("channel", Json::s(&channel)),
                ("release", Json::s(&d.release)),
                ("run", d.run.as_deref().map_or(Json::Null, Json::s)),
                ("target", d.target.as_deref().map_or(Json::Null, Json::s)),
                (
                    "desired",
                    Json::Obj(
                        d.services
                            .iter()
                            .map(|(s, v)| (s.clone(), Json::s(v)))
                            .collect(),
                    ),
                ),
            ]),
            None => Json::obj(vec![
                ("installId", Json::s(&install_id)),
                ("channel", Json::s(&channel)),
                ("release", Json::Null),
                ("run", Json::Null),
                ("target", Json::Null),
                ("desired", Json::Null),
            ]),
        };
        (outcome::ACCEPTED, body.encode())
    }

    fn create_release(&self, peer: &WorkloadIdentity, req: &Json, now: u64) -> (u8, String) {
        let parsed = (|| -> Result<(String, BTreeMap<String, String>), String> {
            let at = "the release";
            only_keys(object(req, at)?, at, &["version", "artefacts"])?;
            let version = text(req, at, "version")?.to_string();
            check_name(&version, "a release version")?;
            let artefacts = match req.get("artefacts") {
                None | Some(Json::Null) => self
                    .rings
                    .services()
                    .into_iter()
                    .map(|s| (s.to_string(), version.clone()))
                    .collect(),
                Some(map) => {
                    let fields = object(map, "artefacts")?;
                    no_repeated_keys(fields, "artefacts")?;
                    fields
                        .iter()
                        .map(|(s, v)| match v {
                            Json::Str(v) => Ok((s.clone(), v.clone())),
                            _ => Err(format!("artefacts.{s} must be a version string")),
                        })
                        .collect::<Result<_, String>>()?
                }
            };
            Ok((version, artefacts))
        })();
        let (version, artefacts) = match parsed {
            Ok(p) => p,
            Err(e) => return (outcome::MALFORMED, e),
        };
        let release = Release {
            version: version.clone(),
            artefacts,
            created_by: peer.id.0.clone(),
            created_at: now,
        };
        // The shape first (every ring service, nothing else), without saving anything.
        let mut trial = lock(&self.releases).state().clone();
        if let Err(e) = trial.create_release(release.clone(), &self.rings) {
            return rollout_error(e);
        }
        // Every artefact must be held, verified, and pass the whole gate again now.
        let mut missing = Vec::new();
        let mut unverified = Vec::new();
        {
            let registry = lock(&self.registry);
            for (id, v) in &release.artefacts {
                match registry.record(id, v) {
                    Some(r) if r.state.is_ok() => {}
                    Some(r) => unverified.push(format!(
                        "{id}@{v} is unverifiable ({})",
                        r.state.as_ref().err().map_or("", String::as_str)
                    )),
                    None => match registry
                        .rejections()
                        .iter()
                        .rev()
                        .find(|x| &x.id == id && &x.version == v)
                    {
                        Some(x) => {
                            unverified.push(format!("{id}@{v} was rejected ({})", x.reason.code()))
                        }
                        None => missing.push(format!("{id}@{v} is not in the registry")),
                    },
                }
            }
        }
        if missing.is_empty() && unverified.is_empty() {
            for (id, v) in &release.artefacts {
                if let Err((_, message)) =
                    self.fetch_verified(peer, ACTION_CREATE_RELEASE, id, v, Recheck::Full, now)
                {
                    unverified.push(format!("{id}@{v}: {message}"));
                }
            }
        }
        if !missing.is_empty() || !unverified.is_empty() {
            let code = if unverified.is_empty() {
                outcome::NOT_FOUND
            } else {
                outcome::REJECTED
            };
            let problems: Vec<String> = unverified.into_iter().chain(missing).collect();
            let audited = lock(&self.governance).record_event(
                &peer.id.0,
                "CreateRelease refused: artefacts not verified",
                &format!("release:{version}"),
                now,
            );
            let mut message = format!(
                "release {version} refused: only verified artefacts may be released: {}",
                problems.join("; ")
            );
            if audited.is_err() {
                message.push_str(" (not audited)");
            }
            return (code, message);
        }
        let created = self.change(peer, now, |state| {
            state.create_release(release.clone(), &self.rings)?;
            Ok(((), Vec::new()))
        });
        match created {
            Ok((_, state, _)) => {
                let audited = lock(&self.governance).record_event(
                    &peer.id.0,
                    "CreateRelease created",
                    &format!("release:{version}"),
                    now,
                );
                let mut body = release_json(&state.releases[&version]);
                if audited.is_err() {
                    if let Json::Obj(fields) = &mut body {
                        fields.push(("audited".into(), Json::Bool(false)));
                    }
                }
                (outcome::ACCEPTED, body.encode())
            }
            Err(answer) => answer,
        }
    }

    fn list_releases(&self, _: &WorkloadIdentity, req: &Json, _: u64) -> (u8, String) {
        if let Err(e) = only_keys(
            object(req, "the request").unwrap_or(&[]),
            "the request",
            &[],
        ) {
            return (outcome::MALFORMED, e);
        }
        let store = lock(&self.releases);
        let state = store.state();
        let mut releases: Vec<&Release> = state.releases.values().collect();
        releases.sort_by(|a, b| (b.created_at, &b.version).cmp(&(a.created_at, &a.version)));
        let channels = state
            .channels
            .values()
            .map(|c| {
                let desired = state.desired(&c.name, &self.rings);
                Json::obj(vec![
                    ("name", Json::s(&c.name)),
                    ("current", Json::s(&c.current)),
                    (
                        "previous",
                        c.previous.as_deref().map_or(Json::Null, Json::s),
                    ),
                    (
                        "run",
                        desired
                            .as_ref()
                            .and_then(|d| d.run.as_deref())
                            .map_or(Json::Null, Json::s),
                    ),
                    (
                        "target",
                        desired
                            .as_ref()
                            .and_then(|d| d.target.as_deref())
                            .map_or(Json::Null, Json::s),
                    ),
                    (
                        "desired",
                        desired.map_or(Json::Null, |d| {
                            Json::Obj(
                                d.services
                                    .iter()
                                    .map(|(s, v)| (s.clone(), Json::s(v)))
                                    .collect(),
                            )
                        }),
                    ),
                ])
            })
            .collect();
        let body = Json::obj(vec![
            (
                "releases",
                Json::Arr(releases.into_iter().map(release_json).collect()),
            ),
            ("channels", Json::Arr(channels)),
            ("rings", rings_json(&self.rings)),
        ]);
        (outcome::ACCEPTED, body.encode())
    }

    fn promote(&self, peer: &WorkloadIdentity, req: &Json, now: u64) -> (u8, String) {
        let parsed = only_keys(
            object(req, "the promotion").unwrap_or(&[]),
            "the promotion",
            &["channel", "release"],
        )
        .and_then(|_| {
            Ok((
                text(req, "the promotion", "channel")?.to_string(),
                text(req, "the promotion", "release")?.to_string(),
            ))
        });
        let (channel, version) = match parsed {
            Ok(p) => p,
            Err(e) => return (outcome::MALFORMED, e),
        };
        // The release's artefacts must still be verified: one marked unverifiable since
        // the release was made stops the promotion.
        let artefacts = lock(&self.releases)
            .state()
            .releases
            .get(&version)
            .map(|r| r.artefacts.clone());
        if let Some(artefacts) = artefacts {
            let registry = lock(&self.registry);
            let bad: Vec<String> = artefacts
                .iter()
                .filter(|(id, v)| !registry.record(id, v).is_some_and(|r| r.state.is_ok()))
                .map(|(id, v)| format!("{id}@{v}"))
                .collect();
            if !bad.is_empty() {
                return (
                    outcome::REJECTED,
                    format!(
                        "release {version} has artefacts that no longer verify: {}",
                        bad.join(", ")
                    ),
                );
            }
        }
        let rings = &self.rings;
        let by = peer.id.0.clone();
        match self.change(peer, now, |state| {
            state.promote(&channel, &version, rings, &by, now)
        }) {
            Ok((id, state, audited)) => run_answer(state.run(&id), audited),
            Err(answer) => answer,
        }
    }

    fn rollback(&self, peer: &WorkloadIdentity, req: &Json, now: u64) -> (u8, String) {
        let parsed = only_keys(
            object(req, "the rollback").unwrap_or(&[]),
            "the rollback",
            &["channel", "reason", "to"],
        )
        .and_then(|_| {
            let to = match req.get("to") {
                None | Some(Json::Null) => None,
                Some(_) => Some(text(req, "the rollback", "to")?.to_string()),
            };
            let reason = text(req, "the rollback", "reason")?.to_string();
            if reason.chars().count() > 500 {
                return Err("the rollback: \"reason\" is at most 500 characters".into());
            }
            Ok((
                text(req, "the rollback", "channel")?.to_string(),
                reason,
                to,
            ))
        });
        let (channel, reason, to) = match parsed {
            Ok(p) => p,
            Err(e) => return (outcome::MALFORMED, e),
        };
        let by = peer.id.0.clone();
        match self.change(peer, now, |state| {
            state.rollback(&channel, to.as_deref(), &reason, &by, now)
        }) {
            Ok((id, state, audited)) => run_answer(state.run(&id), audited),
            Err(answer) => answer,
        }
    }

    fn list_rollouts(&self, _: &WorkloadIdentity, req: &Json, _: u64) -> (u8, String) {
        let parsed = (|| -> Result<(Option<String>, usize), String> {
            only_keys(
                object(req, "the request")?,
                "the request",
                &["channel", "limit"],
            )?;
            let channel = match req.get("channel") {
                None | Some(Json::Null) => None,
                Some(_) => Some(text(req, "the request", "channel")?.to_string()),
            };
            let limit = match req.get("limit") {
                None => MAX_ROLLOUTS_PAGE,
                Some(_) => usize::try_from(count(req, "the request", "limit")?)
                    .unwrap_or(MAX_ROLLOUTS_PAGE)
                    .min(MAX_ROLLOUTS_PAGE),
            };
            Ok((channel, limit))
        })();
        let (channel, limit) = match parsed {
            Ok(p) => p,
            Err(e) => return (outcome::MALFORMED, e),
        };
        let store = lock(&self.releases);
        let runs: Vec<Json> = store
            .state()
            .runs
            .iter()
            .rev()
            .filter(|r| channel.as_deref().is_none_or(|c| r.channel == c))
            .take(limit)
            .map(run_json)
            .collect();
        (
            outcome::ACCEPTED,
            Json::obj(vec![("rollouts", Json::Arr(runs))]).encode(),
        )
    }

    fn get_rollout(&self, _: &WorkloadIdentity, req: &Json, _: u64) -> (u8, String) {
        let id = match only_keys(
            object(req, "the request").unwrap_or(&[]),
            "the request",
            &["id"],
        )
        .and_then(|_| text(req, "the request", "id"))
        {
            Ok(id) => id.to_string(),
            Err(e) => return (outcome::MALFORMED, e),
        };
        match lock(&self.releases).state().run(&id) {
            Some(run) => (outcome::ACCEPTED, run_json(run).encode()),
            None => (outcome::NOT_FOUND, format!("no rollout {id}")),
        }
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
            outcome::NOT_FOUND => "not-found",
            outcome::REJECTED => "rejected",
            outcome::CONFLICT => "conflict",
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

/// The outcome and message for a registry error.
fn store_error(e: StoreError) -> (u8, String) {
    let code = match &e {
        StoreError::Malformed(_) => outcome::MALFORMED,
        StoreError::NotFound(_) => outcome::NOT_FOUND,
        StoreError::Conflict(_) => outcome::CONFLICT,
        StoreError::Refused(_) | StoreError::Unverifiable(_) | StoreError::VerifyFailed(_) => {
            outcome::REJECTED
        }
        StoreError::Busy(_) | StoreError::Io(_) => outcome::FAILED,
    };
    (code, e.to_string())
}

/// Each channel and its current release.
fn channel_currents(state: &Rollouts) -> Vec<(String, String)> {
    state
        .channels
        .values()
        .map(|c| (c.name.clone(), c.current.clone()))
        .collect()
}

/// The outcome and message for a refused orchestrator operation.
fn rollout_error(e: RolloutError) -> (u8, String) {
    let code = match &e {
        RolloutError::NotFound(_) => outcome::NOT_FOUND,
        RolloutError::Conflict(_) => outcome::CONFLICT,
        RolloutError::Invalid(_) => outcome::MALFORMED,
    };
    (code, e.to_string())
}

/// A run as the answer to a promotion or rollback.
fn run_answer(run: Option<&RolloutRun>, audited: bool) -> (u8, String) {
    match run {
        Some(run) => {
            let mut body = run_json(run);
            if !audited {
                if let Json::Obj(fields) = &mut body {
                    fields.push(("audited".into(), Json::Bool(false)));
                }
            }
            (outcome::ACCEPTED, body.encode())
        }
        None => (outcome::FAILED, "the run is gone".into()),
    }
}

/// A whole number field of at least 0.
fn count(req: &Json, at: &str, key: &str) -> Result<u64, String> {
    match req.get(key) {
        Some(Json::Int(n)) if *n >= 0 => Ok(*n as u64),
        _ => Err(format!("{at}: \"{key}\" must be a whole number")),
    }
}

fn sha256_field(item: &Json, at: &str, key: &str) -> Result<[u8; 32], String> {
    let hex = text(item, at, key)?;
    attest::hex::unhex(hex)
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .ok_or_else(|| format!("{at}: \"{key}\" must be 64 hex digits"))
}

/// Parse a [`REGISTER_ARTEFACT`] request strictly.
fn parse_new_upload(req: &Json) -> Result<NewUpload, String> {
    let at = "the registration";
    only_keys(
        object(req, at)?,
        at,
        &["manifest", "sbom", "signature", "signer", "size"],
    )?;
    let m = req
        .get("manifest")
        .ok_or("the registration: missing \"manifest\"")?;
    let mat = "the manifest";
    only_keys(
        object(m, mat)?,
        mat,
        &["id", "version", "component", "payloadSha256", "sbomSha256"],
    )?;
    let manifest = ArtefactManifest {
        id: text(m, mat, "id")?.to_string(),
        version: text(m, mat, "version")?.to_string(),
        component: text(m, mat, "component")?.to_string(),
        payload_sha256: sha256_field(m, mat, "payloadSha256")?,
        sbom_sha256: sha256_field(m, mat, "sbomSha256")?,
    };
    // Base64, so the SBOM's size on the wire does not depend on how JSON escapes it.
    let sbom = match req.get("sbom") {
        Some(Json::Str(s)) => b64::decode(s)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or_else(|| format!("{at}: \"sbom\" must be base64 of UTF-8"))?,
        _ => return Err(format!("{at}: \"sbom\" must be a base64 string")),
    };
    let signature = attest::hex::unhex(text(req, at, "signature")?)
        .ok_or_else(|| format!("{at}: \"signature\" must be hex"))?;
    Ok(NewUpload {
        manifest,
        sbom,
        signature: Signature::new(BIFROST_SIG_ALG, signature),
        signer: text(req, at, "signer")?.to_string(),
        size: count(req, at, "size")?,
    })
}

fn manifest_json(m: &ArtefactManifest) -> Json {
    Json::obj(vec![
        ("id", Json::s(&m.id)),
        ("version", Json::s(&m.version)),
        ("component", Json::s(&m.component)),
        (
            "payloadSha256",
            Json::s(&attest::hex::hex(&m.payload_sha256)),
        ),
        ("sbomSha256", Json::s(&attest::hex::hex(&m.sbom_sha256))),
    ])
}

/// A held artefact as listed: `verified`, or `unverifiable` with the reason.
fn record_json(r: &Record) -> Json {
    let m = &r.manifest;
    let mut fields = vec![
        ("id", Json::s(&m.id)),
        ("version", Json::s(&m.version)),
        ("component", Json::s(&m.component)),
        (
            "status",
            Json::s(if r.state.is_ok() {
                "verified"
            } else {
                "unverifiable"
            }),
        ),
        ("signer", Json::s(&r.signer)),
        (
            "payloadSha256",
            Json::s(&attest::hex::hex(&m.payload_sha256)),
        ),
        ("sbomSha256", Json::s(&attest::hex::hex(&m.sbom_sha256))),
        ("size", Json::Int(i64::try_from(r.size).unwrap_or(i64::MAX))),
        ("registeredBy", Json::s(&r.registered_by)),
        (
            "registeredAt",
            Json::Int(i64::try_from(r.registered_at).unwrap_or(i64::MAX)),
        ),
    ];
    if let Err(reason) = &r.state {
        fields.push(("detail", Json::s(reason)));
    }
    Json::obj(fields)
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
