//! # heimdall-feed-server — the Heimdall feed gateway as a mesh service
//!
//! Feed sources submit signed observations over PQ-mTLS; analysts read the fused
//! operating picture. [FR-H-1] [FR-H-3] [NFR-PQ-3] [NFR-SEC-1]
//!
//! An observation passes four gates before it can change the picture:
//!
//! 1. the source's mesh certificate opens the channel (the gateway's PDP);
//! 2. policy permits its roles to `Ingest`, and the decision is audited;
//! 3. the observation names the source's own feed — a feed's identity is the service
//!    its certificate names, so one source cannot speak for another;
//! 4. its signature verifies against the key that certificate binds, through the
//!    feed gateway's admission check — the only way to a `VerifiedObservation`.
//!
//! A refusal at 3 or 4 is audited too, and so is a fifth: an observation whose id has
//! already been fused is a replay and is refused. Admitted observations are fused and
//! the picture is sealed at rest after each one, so a restart recovers it. [RC-13]
//!
//! With a track time-to-live ([`FeedService::set_track_ttl`]), entities not seen for
//! that long are removed from the picture after an ingest, at most once every
//! [`EXPIRY_SWEEP_SECS`], and each sweep that removes any is audited. The live picture
//! is current state; the feed ledger keeps the history.
//!
//! Each fused observation with a numeric `lat` and `lon` also adds a point to its
//! entity's track history ([`heimdall_store::DurableHistory`], sealed at rest), which
//! analysts read with [`READ_HISTORY`]. History is kept for a retention window
//! ([`FeedService::set_history_retention`]) — a track that expires from the picture
//! keeps its history until then — and is pruned and compacted at most once every
//! [`HISTORY_MAINTAIN_SECS`]. [FR-H-1] [RC-13]
//!
//! A time-to-live can differ by kind ([`FeedService::set_kind_ttls`], from `ttl.json`;
//! see [`ttl_file`]). Where an observation's position came from (`pos_source`) ranks
//! it: an ADS-B position outranks a derived one for a minute (see
//! [`heimdall_fusion::engine`]), and only positions the entity adopted become history.
//!
//! An [`AlertBook`] (configured by `alerts.json`; see [`alerts`]) watches the picture:
//! classification labels, tracks that go dark, and feeds that disagree on a position.
//! Each raise and clear is audited (`RaiseAlert`, `ClearAlert`); analysts read alerts
//! with [`READ_ALERTS`] and acknowledge them with [`ACKNOWLEDGE_ALERT`], both
//! authorised and audited. [FR-H-1] [FR-H-2] [FR-H-4]
//!
//! Responses are one outcome byte ([`outcome`]) followed by UTF-8: a message, or for
//! [`READ_PICTURE`] the picture as JSON, for [`READ_HISTORY`] a page of history as
//! JSON, for [`READ_ALERTS`] the alerts and for [`ACKNOWLEDGE_ALERT`] the alert.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use audit_writer::LedgerStore;
use gateway::{RequestHandler, Router};
use governance::{GovContext, Governance};
use heimdall_feed_gateway::{
    decode_signed_observation, FeedGateway, FeedRegistry, VerifiedObservation,
};
use heimdall_fusion::FusionEngine;
use heimdall_picture::{
    Attr, ClassificationSource, EntityId, FeedId, Picture, PictureDelta, TrackedEntity,
};
use heimdall_store::{
    position_of, DurableHistory, DurablePicture, HistoryQuery, HistoryRecord, StoreError,
};
use policy::{ClassificationLevel, Clearance, Purpose, Resource};
use pq_mtls::WorkloadIdentity;
use pqc_agility::SharedProvider;
use service_runtime::json::Json;
use telemetry::MetricsRegistry;

pub mod alerts;
pub mod rules_file;
pub mod ttl_file;
pub use alerts::{parse_alerts, Alert, AlertBook, AlertChange, AlertConfig};
use heimdall_decision_support::OptionProvider;
pub use rules_file::parse_rules;
pub use ttl_file::{describe_ttl, parse_ttl};

/// The most often stale tracks are swept, in seconds of the service clock.
pub const EXPIRY_SWEEP_SECS: u64 = 30;
/// The most often track history is pruned and compacted, in seconds.
pub const HISTORY_MAINTAIN_SECS: u64 = 300;
/// How long track history is kept by default, in seconds (a day).
pub const DEFAULT_HISTORY_RETENTION: u64 = 86_400;
/// The most bytes of tracks one [`READ_HISTORY`] response carries by default; transport
/// frames are capped at 1 MiB, and a caller pages for the rest.
pub const HISTORY_PAGE_BYTES: usize = 900 * 1024;
/// How many recent observation ids are remembered to refuse replays. Older ids fall
/// out first; ids are also forgotten on restart, which the ledger records.
pub const REPLAY_MEMORY: usize = 200_000;

/// The method a source submits one encoded signed observation with.
pub const INGEST: &str = "Ingest";
/// The method a source submits many encoded signed observations with, framed by
/// [`encode_batch`].
pub const INGEST_BATCH: &str = "IngestBatch";
/// The method an analyst reads the picture with, a page at a time; its payload is
/// empty (the first page) or a JSON query (see [`FeedService::read_picture`]).
pub const READ_PICTURE: &str = "ReadPicture";
/// The most bytes of entities one [`READ_PICTURE`] page carries by default; transport
/// frames are capped at 1 MiB, and a caller pages for the rest. [FR-H-1]
pub const PICTURE_PAGE_BYTES: usize = 900 * 1024;
/// The method an analyst reads track history with; its payload is a JSON query
/// (see [`FeedService::read_history`]).
pub const READ_HISTORY: &str = "ReadHistory";
/// The method an analyst reads alerts with; its payload is `{"state": "open"|"all"}`
/// (see [`FeedService::read_alerts`]).
pub const READ_ALERTS: &str = "ReadAlerts";
/// The method an analyst acknowledges an alert with; its payload is `{"id", "by"}`
/// (see [`FeedService::acknowledge_alert`]).
pub const ACKNOWLEDGE_ALERT: &str = "AcknowledgeAlert";
/// Method: ask decision support (Mimir, via the A.8 prompt) for ranked, cited courses
/// of action about an alert or an entity. Advisory only. [FR-H-4] [NFR-AI-1]
pub const ADVISE_OPTIONS: &str = "AdviseOptions";
/// Method: ask the Mimir core agent (A.1) a question over the picture in an area,
/// answered from cited evidence. Advisory only. [FR-MI-3] [FR-H-4]
pub const ASK_PICTURE: &str = "AskPicture";
/// The policy action an alert read is authorised as.
pub const ACTION_READ_ALERTS: &str = "ReadAlerts";
/// The policy action an acknowledgement is authorised as.
pub const ACTION_ACKNOWLEDGE_ALERT: &str = "AcknowledgeAlert";
/// Policy action for [`ADVISE_OPTIONS`].
pub const ACTION_ADVISE_OPTIONS: &str = "AdviseOptions";
/// Policy action for [`ASK_PICTURE`].
pub const ACTION_ASK_PICTURE: &str = "AskPicture";
/// The most entities a question's grounding carries.
pub const ASK_MAX_ENTITIES: usize = 20;
/// The most entities a decision-support picture carries (the subject and its nearest).
pub const ADVISE_MAX_ENTITIES: usize = 16;
/// How far from the subject decision support looks for context, in km.
pub const ADVISE_RADIUS_KM: f64 = 80.0;
/// The most bytes of alerts one [`READ_ALERTS`] response carries (under 900 KiB);
/// past it the oldest are left out and `"truncated"` is true.
pub const ALERTS_PAGE_BYTES: usize = 900 * 1024 - 1024;
/// The policy action a submission is authorised as.
pub const ACTION_INGEST: &str = "Ingest";
/// The policy action a picture read is authorised as.
pub const ACTION_READ_PICTURE: &str = "ReadPicture";
/// The policy action a history read is authorised as.
pub const ACTION_READ_HISTORY: &str = "ReadHistory";

/// The first byte of every feed response.
pub mod outcome {
    /// Done: fused, or the picture follows.
    pub const ACCEPTED: u8 = 0;
    /// Refused by policy, from another feed, or not verified.
    pub const DENIED: u8 = 1;
    /// The payload did not decode.
    pub const MALFORMED: u8 = 2;
    /// It could not be audited or persisted.
    pub const FAILED: u8 = 3;
    /// What it names does not exist (an unknown alert id).
    pub const NOT_FOUND: u8 = 4;
}

/// The fused picture and its sealed, durable copy, kept in step.
pub struct PictureState {
    engine: FusionEngine,
    durable: DurablePicture,
    history: Option<DurableHistory>,
    alerts: AlertBook,
    seen: BTreeSet<String>,
    seen_order: VecDeque<String>,
}

impl PictureState {
    /// Resume fusion over a recovered picture: the engine starts from everything the
    /// durable picture holds, so later observations extend the recovered tracks.
    pub fn recover(durable: DurablePicture) -> Self {
        Self::recover_with(durable, FusionEngine::new())
    }

    /// [`PictureState::recover`] with a configured engine (a classifier, a layer
    /// policy). Recovered entities keep the classification they were sealed with until
    /// their next observation reclassifies them.
    pub fn recover_with(durable: DurablePicture, mut engine: FusionEngine) -> Self {
        PictureDelta::snapshot(durable.picture()).merge_into(engine.picture_mut());
        for layer in durable.picture().layers() {
            if !durable.picture().is_layer_visible(&layer.id) {
                engine.picture_mut().set_layer_visible(&layer.id, false);
            }
        }
        Self {
            engine,
            durable,
            history: None,
            alerts: AlertBook::in_memory(AlertConfig::default()),
            seen: BTreeSet::new(),
            seen_order: VecDeque::new(),
        }
    }

    /// Keep track history in `history`: every positioned observation fused from now on
    /// adds a point to it. [FR-H-1]
    pub fn with_history(mut self, history: DurableHistory) -> Self {
        self.history = Some(history);
        self
    }

    /// Keep alerts in `book` (by default an in-memory book that raises none).
    /// [FR-H-2] [FR-H-4]
    pub fn with_alerts(mut self, book: AlertBook) -> Self {
        self.alerts = book;
        self
    }

    /// The alerts.
    pub fn alerts(&self) -> &AlertBook {
        &self.alerts
    }

    /// The alerts, to change.
    pub fn alerts_mut(&mut self) -> &mut AlertBook {
        &mut self.alerts
    }

    /// The track history, if one is kept.
    pub fn history(&self) -> Option<&DurableHistory> {
        self.history.as_ref()
    }

    /// The point a fused observation adds to its entity's history, if it is positioned.
    fn history_record(
        &self,
        observation: &VerifiedObservation,
        id: &EntityId,
    ) -> Option<HistoryRecord> {
        let o = observation.observation();
        let (lat, lon, alt_m) = position_of(&o.attrs)?;
        let entity = self.engine.picture().entity(id)?;
        Some(HistoryRecord {
            entity: id.0.clone(),
            kind: entity.kind.clone(),
            layer: entity.layer.0.clone(),
            t: o.observed_at,
            lat,
            lon,
            alt_m,
        })
    }

    /// Record points in the history, if one is kept.
    fn record_history(&mut self, records: &[HistoryRecord], now: u64) -> Result<(), StoreError> {
        match self.history.as_mut() {
            Some(history) => history.record(records, now),
            None => Ok(()),
        }
    }

    /// Whether an observation with this id has already been fused (since start).
    pub fn already_fused(&self, observation_id: &str) -> bool {
        self.seen.contains(observation_id)
    }

    /// Remove entities last seen before `cutoff`, from the live and the sealed picture.
    pub fn expire_before(&mut self, cutoff: u64) -> Result<Vec<EntityId>, StoreError> {
        self.expire(cutoff, |_| 0)
    }

    /// Remove every entity whose kind's time-to-live (`ttl_of(kind)` seconds; 0 keeps
    /// it for ever) has passed since it was last seen, from the live and the sealed
    /// picture in one write, and tell the alert book. [FR-H-1] [FR-H-2]
    pub fn expire_by_kind(
        &mut self,
        now: u64,
        ttl_of: impl Fn(&str) -> u64,
    ) -> Result<Vec<EntityId>, StoreError> {
        self.expire(now, |kind| match ttl_of(kind) {
            0 => u64::MAX,
            ttl => ttl,
        })
    }

    /// Expire entities last seen before `now - age(kind)` (an age of 0: `now` itself;
    /// `u64::MAX`: never).
    fn expire(&mut self, now: u64, age: impl Fn(&str) -> u64) -> Result<Vec<EntityId>, StoreError> {
        let stale: Vec<EntityId> = self
            .engine
            .picture()
            .entities()
            .filter(|e| {
                let age = age(&e.kind);
                age != u64::MAX && e.track.last_seen < now.saturating_sub(age)
            })
            .map(|e| e.id.clone())
            .collect();
        if stale.is_empty() {
            return Ok(stale);
        }
        let gone = self.engine.picture_mut().remove_entities(&stale);
        let removed = self.durable.remove_entities(&stale)?;
        self.alerts.expired(&gone, now);
        let _ = self.alerts.flush();
        Ok(removed)
    }

    /// The alert changes since last taken, for the caller to audit.
    pub fn take_alert_changes(&mut self) -> Vec<AlertChange> {
        self.alerts.take_changes()
    }

    fn remember(&mut self, observation_id: &str) {
        if self.seen.insert(observation_id.to_string()) {
            self.seen_order.push_back(observation_id.to_string());
            while self.seen_order.len() > REPLAY_MEMORY {
                if let Some(old) = self.seen_order.pop_front() {
                    self.seen.remove(&old);
                }
            }
        }
    }

    /// The current picture.
    pub fn picture(&self) -> &Picture {
        self.engine.picture()
    }

    /// Fuse verified observations in order and seal the picture once for all of them;
    /// their positions go to the history in one append. `now` is the service clock.
    pub fn fuse_many(
        &mut self,
        observations: &[VerifiedObservation],
        now: u64,
    ) -> Result<(), StoreError> {
        if observations.is_empty() {
            return Ok(());
        }
        let mut changed = BTreeSet::new();
        let mut records = Vec::new();
        for observation in observations {
            let fused = self.engine.fuse(observation);
            if fused.position_adopted {
                records.extend(self.history_record(observation, &fused.id));
            }
            if let Some(entity) = self.engine.picture().entity(&fused.id) {
                self.alerts.observed(entity, observation.observation(), now);
            }
            changed.insert(fused.id);
        }
        for id in &changed {
            if let Some(entity) = self.engine.picture().entity(id) {
                self.alerts.entity_updated(entity, now);
            }
        }
        let layers: Vec<_> = self.engine.picture().layers().cloned().collect();
        let entities: Vec<_> = changed
            .iter()
            .filter_map(|id| self.engine.picture().entity(id).cloned())
            .collect();
        self.durable.upsert_all(layers, entities)?;
        self.record_history(&records, now)?;
        for observation in observations {
            self.remember(&observation.observation().id.0);
        }
        // An alert book that fails to seal is written again on its next change.
        let _ = self.alerts.flush();
        Ok(())
    }

    /// Fuse a verified observation and seal the entity it changed, and its position
    /// in the history. `now` is the service clock.
    pub fn fuse(
        &mut self,
        observation: &VerifiedObservation,
        now: u64,
    ) -> Result<EntityId, StoreError> {
        let fused = self.engine.fuse(observation);
        let id = fused.id.clone();
        let Some(entity) = self.engine.picture().entity(&id).cloned() else {
            return Ok(id);
        };
        if !self
            .durable
            .picture()
            .layers()
            .any(|l| l.id == entity.layer)
        {
            if let Some(layer) = self
                .engine
                .picture()
                .layers()
                .find(|l| l.id == entity.layer)
                .cloned()
            {
                self.durable.ensure_layer(layer)?;
            }
        }
        self.alerts
            .observed(&entity, observation.observation(), now);
        self.alerts.entity_updated(&entity, now);
        self.durable.upsert_entity(entity)?;
        let record: Vec<_> = if fused.position_adopted {
            self.history_record(observation, &id).into_iter().collect()
        } else {
            Vec::new()
        };
        self.record_history(&record, now)?;
        // Remembered only once durable, so a submission that failed to persist can be
        // retried rather than refused as a replay.
        self.remember(&observation.observation().id.0);
        let _ = self.alerts.flush();
        Ok(id)
    }
}

/// The feed service: the picture behind the governance spine.
pub struct FeedService<S: LedgerStore> {
    state: Mutex<PictureState>,
    governance: Mutex<Governance<S>>,
    provider: SharedProvider,
    resource: Resource,
    purpose: Purpose,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    metrics: Arc<MetricsRegistry>,
    track_ttl: AtomicU64,
    kind_ttls: Mutex<BTreeMap<String, u64>>,
    last_sweep: AtomicU64,
    history_retention: AtomicU64,
    last_history_maintenance: AtomicU64,
    history_page_bytes: AtomicUsize,
    picture_page_bytes: AtomicUsize,
    /// Decision support (Mimir), when configured: [`ADVISE_OPTIONS`] needs it.
    advisor: Mutex<Option<Arc<dyn OptionProvider + Send + Sync>>>,
}

impl<S: LedgerStore + Send + 'static> FeedService<S> {
    /// Assemble the service. `resource` is what requests are authorised against and
    /// `purpose` the purpose they are audited under; `provider` verifies feed
    /// signatures.
    pub fn new(
        state: PictureState,
        governance: Governance<S>,
        provider: SharedProvider,
        resource: Resource,
        purpose: Purpose,
        clock: Arc<dyn Fn() -> u64 + Send + Sync>,
        metrics: Arc<MetricsRegistry>,
    ) -> Arc<Self> {
        metrics.describe(
            "feed_requests_total",
            "Feed requests, by method and outcome.",
        );
        metrics.describe(
            "feed_history_points_pruned_total",
            "Track-history points removed for being older than the retention window.",
        );
        metrics.describe(
            "feed_alerts_total",
            "Alert changes, by action (RaiseAlert, ClearAlert).",
        );
        metrics.describe(
            "feed_tracks_expired_total",
            "Tracks removed from the picture for not being seen within the time-to-live.",
        );
        Arc::new(Self {
            state: Mutex::new(state),
            governance: Mutex::new(governance),
            provider,
            resource,
            purpose,
            clock,
            metrics,
            track_ttl: AtomicU64::new(0),
            kind_ttls: Mutex::new(BTreeMap::new()),
            last_sweep: AtomicU64::new(0),
            history_retention: AtomicU64::new(DEFAULT_HISTORY_RETENTION),
            last_history_maintenance: AtomicU64::new(0),
            history_page_bytes: AtomicUsize::new(HISTORY_PAGE_BYTES),
            picture_page_bytes: AtomicUsize::new(PICTURE_PAGE_BYTES),
            advisor: Mutex::new(None),
        })
    }

    /// Serve [`ADVISE_OPTIONS`] through `advisor` (typically Mimir over PQ-mTLS).
    /// Without one, the method answers that decision support is not configured.
    pub fn set_advisor(&self, advisor: Arc<dyn OptionProvider + Send + Sync>) {
        *lock(&self.advisor) = Some(advisor);
    }

    /// Remove entities not seen for `seconds` (0, the default, keeps them for ever).
    pub fn set_track_ttl(&self, seconds: u64) {
        self.track_ttl.store(seconds, Ordering::Relaxed);
    }

    /// The most bytes of entities one [`READ_PICTURE`] page carries (default
    /// [`PICTURE_PAGE_BYTES`]; smaller only to exercise paging). [FR-H-1]
    pub fn set_picture_page_bytes(&self, bytes: usize) {
        self.picture_page_bytes
            .store(bytes.max(1), Ordering::Relaxed);
    }

    /// Time-to-live by exact kind, overriding [`FeedService::set_track_ttl`] for those
    /// kinds (0 keeps a kind for ever). [FR-H-1]
    pub fn set_kind_ttls(&self, ttls: BTreeMap<String, u64>) {
        *lock(&self.kind_ttls) = ttls;
    }

    /// Keep track history for `seconds` (default [`DEFAULT_HISTORY_RETENTION`]); older
    /// points are pruned, and a [`READ_HISTORY`] window may be no longer than this.
    pub fn set_history_retention(&self, seconds: u64) {
        self.history_retention.store(seconds, Ordering::Relaxed);
    }

    /// The most bytes of tracks one [`READ_HISTORY`] response carries (default
    /// [`HISTORY_PAGE_BYTES`]; smaller only to exercise paging).
    pub fn set_history_page_bytes(&self, bytes: usize) {
        self.history_page_bytes
            .store(bytes.max(1), Ordering::Relaxed);
    }

    /// Expire stale tracks and maintain the history, each when it is due; prune old
    /// cleared alerts; and audit every alert change since the last sweep.
    fn sweep(&self, now: u64) {
        self.expire_tracks(now);
        self.maintain_history(now);
        {
            let mut state = lock(&self.state);
            if state.alerts.prune(now) > 0 {
                let _ = state.alerts.flush();
            }
        }
        self.audit_alert_changes(now);
    }

    /// Record each alert change in the ledger, one entry per change. [FR-H-4]
    fn audit_alert_changes(&self, now: u64) {
        let changes = lock(&self.state).take_alert_changes();
        if changes.is_empty() {
            return;
        }
        let mut governance = lock(&self.governance);
        for change in &changes {
            let _ =
                governance.record_event("heimdall-feed", change.action(), &change.resource(), now);
            self.metrics
                .inc("feed_alerts_total", &[("action", change.action())]);
        }
    }

    /// Prune history older than the retention window and compact its segments, at most
    /// once every [`HISTORY_MAINTAIN_SECS`]; a pass that prunes points is audited as
    /// `PruneHistory`. [FR-H-1] [RC-13]
    fn maintain_history(&self, now: u64) {
        let last = self.last_history_maintenance.load(Ordering::Relaxed);
        if last != 0 && now.saturating_sub(last) < HISTORY_MAINTAIN_SECS {
            return;
        }
        self.last_history_maintenance.store(now, Ordering::Relaxed);
        let cutoff = now.saturating_sub(self.history_retention.load(Ordering::Relaxed));
        let report = match lock(&self.state).history.as_mut() {
            Some(history) => match history.maintain(now, cutoff) {
                Ok(report) => report,
                Err(_) => return,
            },
            None => return,
        };
        if report.points_pruned > 0 {
            let _ = lock(&self.governance).record_event(
                "heimdall-feed",
                "PruneHistory",
                &format!("points:{}", report.points_pruned),
                now,
            );
            self.metrics.inc_by(
                "feed_history_points_pruned_total",
                &[],
                report.points_pruned as u64,
            );
        }
    }

    /// Sweep stale tracks if a time-to-live is set and the last sweep is old enough;
    /// a sweep that removes anything is audited as `ExpireTracks`.
    fn expire_tracks(&self, now: u64) {
        let ttl = self.track_ttl.load(Ordering::Relaxed);
        let kinds = lock(&self.kind_ttls).clone();
        if (ttl == 0 && kinds.values().all(|t| *t == 0))
            || now.saturating_sub(self.last_sweep.load(Ordering::Relaxed)) < EXPIRY_SWEEP_SECS
        {
            return;
        }
        self.last_sweep.store(now, Ordering::Relaxed);
        let ttl_of = |kind: &str| kinds.get(kind).copied().unwrap_or(ttl);
        let removed = match lock(&self.state).expire_by_kind(now, ttl_of) {
            Ok(removed) => removed,
            Err(_) => return,
        };
        if !removed.is_empty() {
            let _ = lock(&self.governance).record_event(
                "heimdall-feed",
                "ExpireTracks",
                &format!("tracks:{}", removed.len()),
                now,
            );
            self.metrics
                .inc_by("feed_tracks_expired_total", &[], removed.len() as u64);
        }
    }

    /// The router the PQ-mTLS gateway serves: [`INGEST`], [`INGEST_BATCH`],
    /// [`READ_PICTURE`], [`READ_HISTORY`], [`READ_ALERTS`] and [`ACKNOWLEDGE_ALERT`].
    pub fn router(self: &Arc<Self>) -> Router {
        let service = self.clone();
        let ingest: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.ingest(peer, body));
        let service = self.clone();
        let read: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.read_picture(peer, body));
        let service = self.clone();
        let batch: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.ingest_batch(peer, body));
        let service = self.clone();
        let history: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.read_history(peer, body));
        let service = self.clone();
        let alerts: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.read_alerts(peer, body));
        let service = self.clone();
        let ack: Arc<dyn RequestHandler> = Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| {
            service.acknowledge_alert(peer, body)
        });
        Router::new()
            .route(INGEST, ingest)
            .route(INGEST_BATCH, batch)
            .route(READ_PICTURE, read)
            .route(READ_HISTORY, history)
            .route(READ_ALERTS, alerts)
            .route(ACKNOWLEDGE_ALERT, ack)
            .route(ASK_PICTURE, {
                let service = self.clone();
                Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| {
                    service.ask_picture(peer, body)
                }) as Arc<dyn RequestHandler>
            })
            .route(ADVISE_OPTIONS, {
                let service = self.clone();
                Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| {
                    service.advise_options(peer, body)
                }) as Arc<dyn RequestHandler>
            })
    }

    /// A question over the picture: the body is `{"question", "by"}` with the area as
    /// `"boxes": [[w, s, e, n], ..]` and/or `"focus": [lat, lon, km]`. The entities in
    /// the area most relevant to the question (words in their kind, label, name and
    /// facts; open alerts first), up to [`ASK_MAX_ENTITIES`], and an area summary are
    /// the A.1 grounding; every citation in the answer is resolved against them.
    /// Authorised as [`ACTION_ASK_PICTURE`], audited as an `AskPicture` event, and the
    /// picture is not locked while Mimir thinks. [FR-MI-3] [FR-H-4] [NFR-AI-1]
    pub fn ask_picture(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let request = parse_ask_request(body);
        let action = if request.is_ok() {
            ACTION_ASK_PICTURE
        } else {
            "AskPicture:MALFORMED"
        };
        let decision =
            lock(&self.governance).authorize(&self.context(peer), &self.resource, action, now);
        let request = match (decision, request) {
            (Err(_), _) => {
                return self.reply(
                    ASK_PICTURE,
                    outcome::FAILED,
                    "the question could not be audited",
                )
            }
            (Ok(_), Err(e)) => return self.reply(ASK_PICTURE, outcome::MALFORMED, &e),
            (Ok(d), Ok(_)) if !d.permitted => {
                return self.reply(
                    ASK_PICTURE,
                    outcome::DENIED,
                    "policy does not permit this identity to ask Mimir",
                )
            }
            (Ok(_), Ok(r)) => r,
        };
        let Some(advisor) = lock(&self.advisor).clone() else {
            return self.reply(
                ASK_PICTURE,
                outcome::NOT_FOUND,
                "decision support is not configured",
            );
        };
        let (evidence, summary) = {
            let state = lock(&self.state);
            ask_evidence(state.picture(), state.alerts(), &request)
        };
        let refs: Vec<&TrackedEntity> = evidence.iter().collect();
        let started = std::time::Instant::now();
        // What the model plane records for this call: who asked and what about.
        let q: String = request.question.chars().take(60).collect();
        let label = format!(
            "heimdall · question \"{q}{}\" · asked by {}",
            if request.question.chars().count() > 60 {
                "…"
            } else {
                ""
            },
            request.by
        );
        let answer = heimdall_decision_support::answer_question(
            &refs,
            &summary,
            &request.question,
            self.purpose.0.as_str(),
            advisor.as_ref(),
            Some(&label),
        );
        let ms = started.elapsed().as_millis();
        let summary_line = match &answer {
            Ok(a) => format!("citations:{} grounded:{}", a.citations.len(), a.grounded()),
            Err(e) => format!("failed:{e}"),
        };
        let recorded = lock(&self.governance).record_event(
            &peer.id.0,
            "AskPicture",
            &format!(
                "by:{} question_chars:{} evidence:{} {summary_line} ms:{ms}",
                request.by,
                request.question.chars().count(),
                refs.len()
            ),
            now,
        );
        if recorded.is_err() {
            return self.reply(
                ASK_PICTURE,
                outcome::FAILED,
                "the question could not be audited",
            );
        }
        match answer {
            Ok(a) => self.reply(
                ASK_PICTURE,
                outcome::ACCEPTED,
                &ask_json(&a, refs.len(), ms).encode(),
            ),
            Err(e) => self.reply(
                ASK_PICTURE,
                outcome::FAILED,
                &format!("Mimir could not answer: {e}"),
            ),
        }
    }

    /// Decision support: the body is `{"alert": "A-1"}` or `{"entity": "<id>"}`, with
    /// `"by"` (who asks) and an optional `"question"`. The subject and up to
    /// [`ADVISE_MAX_ENTITIES`] of its nearest neighbours within [`ADVISE_RADIUS_KM`]
    /// are sent to the advisor with the A.8 prompt; every citation in the answer is
    /// resolved against that picture. The response is the options, each marked
    /// grounded or not, and is advisory only: nothing is acted on. Authorised as
    /// [`ACTION_ADVISE_OPTIONS`] and audited as an `AdviseOptions` event. The picture
    /// is not locked while the advisor thinks. [FR-H-4] [NFR-AI-1] [AC-H-3]
    pub fn advise_options(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let request = parse_advise_request(body);
        let action = if request.is_ok() {
            ACTION_ADVISE_OPTIONS
        } else {
            "AdviseOptions:MALFORMED"
        };
        let decision =
            lock(&self.governance).authorize(&self.context(peer), &self.resource, action, now);
        let request = match (decision, request) {
            (Err(_), _) => {
                return self.reply(
                    ADVISE_OPTIONS,
                    outcome::FAILED,
                    "the request could not be audited",
                )
            }
            (Ok(_), Err(e)) => return self.reply(ADVISE_OPTIONS, outcome::MALFORMED, &e),
            (Ok(d), Ok(_)) if !d.permitted => {
                return self.reply(
                    ADVISE_OPTIONS,
                    outcome::DENIED,
                    "policy does not permit this identity to ask for decision support",
                )
            }
            (Ok(_), Ok(r)) => r,
        };
        let Some(advisor) = lock(&self.advisor).clone() else {
            return self.reply(
                ADVISE_OPTIONS,
                outcome::NOT_FOUND,
                "decision support is not configured",
            );
        };
        // The subject, its context and the decision point, under the lock; then release it.
        let built = {
            let state = lock(&self.state);
            advise_picture(state.picture(), state.alerts(), &request)
        };
        let (picture, decision_point, subject) = match built {
            Ok(b) => b,
            Err(e) => return self.reply(ADVISE_OPTIONS, outcome::NOT_FOUND, &e),
        };
        let started = std::time::Instant::now();
        let what = match (&request.alert, &request.entity) {
            (Some(a), _) => format!("alert {a}"),
            _ => format!("entity {subject}"),
        };
        let label = format!("heimdall · advice on {what} · asked by {}", request.by);
        let answer = heimdall_decision_support::request_options_labelled(
            &picture,
            &decision_point,
            advisor.as_ref(),
            Some(&label),
        );
        let ms = started.elapsed().as_millis();
        let (body, summary) = match &answer {
            Ok(a) => (
                advise_json(a, &subject, &decision_point, picture.entity_count(), ms),
                format!(
                    "options:{} grounded:{}",
                    a.option_set.options.len(),
                    a.all_grounded()
                ),
            ),
            Err(e) => (Json::Null, format!("failed:{e}")),
        };
        let recorded = lock(&self.governance).record_event(
            &peer.id.0,
            "AdviseOptions",
            &format!("subject:{subject} by:{} {summary} ms:{ms}", request.by),
            now,
        );
        if recorded.is_err() {
            return self.reply(
                ADVISE_OPTIONS,
                outcome::FAILED,
                "the request could not be audited",
            );
        }
        match answer {
            Ok(_) => self.reply(ADVISE_OPTIONS, outcome::ACCEPTED, &body.encode()),
            Err(e) => self.reply(
                ADVISE_OPTIONS,
                outcome::FAILED,
                &format!("decision support failed: {e}"),
            ),
        }
    }

    /// Admit, fuse and persist one signed observation.
    pub fn ingest(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let admitted = self.admit(peer, body, now, &BTreeSet::new());
        let (verified, audit_seq) = match admitted {
            Ok(ok) => ok,
            Err(Refusal { code, message }) => return self.reply(INGEST, code, &message),
        };
        let fused = lock(&self.state).fuse(&verified, now);
        self.sweep(now);
        match fused {
            Ok(entity) => self.reply(
                INGEST,
                outcome::ACCEPTED,
                &format!(
                    "fused into entity {} at audit sequence {audit_seq}",
                    entity.0
                ),
            ),
            Err(e) => self.reply(
                INGEST,
                outcome::FAILED,
                &format!("the picture was not persisted: {e:?}"),
            ),
        }
    }

    /// Admit, fuse and persist a batch of signed observations ([`encode_batch`]). Each
    /// observation passes the same gates as [`FeedService::ingest`] and is authorised
    /// and audited on its own; the admitted ones are fused in order and the picture is
    /// sealed once, so a batch costs one write instead of one per observation. The
    /// response is `{"accepted": N, "refused": [{"index", "code", "message"}]}`.
    pub fn ingest_batch(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let items = match decode_batch(body) {
            Ok(items) => items,
            Err(e) => {
                // Still an authenticated call, so it is still audited.
                let _ = lock(&self.governance).authorize(
                    &self.context(peer),
                    &self.resource,
                    "IngestBatch:MALFORMED",
                    now,
                );
                return self.reply(INGEST_BATCH, outcome::MALFORMED, &e);
            }
        };
        let mut admitted = Vec::with_capacity(items.len());
        let mut pending = BTreeSet::new();
        let mut refused = Vec::new();
        for (index, item) in items.iter().enumerate() {
            match self.admit(peer, item, now, &pending) {
                Ok((verified, _)) => {
                    pending.insert(verified.observation().id.0.clone());
                    admitted.push(verified);
                }
                Err(Refusal { code, message }) => refused.push(Json::obj(vec![
                    ("index", count(index as u64)),
                    ("code", Json::Int(i64::from(code))),
                    ("message", Json::s(&message)),
                ])),
            }
        }
        let fused = lock(&self.state).fuse_many(&admitted, now);
        self.sweep(now);
        if let Err(e) = fused {
            return self.reply(
                INGEST_BATCH,
                outcome::FAILED,
                &format!("the picture was not persisted: {e:?}"),
            );
        }
        let body = Json::obj(vec![
            ("accepted", count(admitted.len() as u64)),
            ("refused", Json::Arr(refused)),
        ]);
        self.reply(INGEST_BATCH, outcome::ACCEPTED, &body.encode())
    }

    /// The five gates every observation passes, each audited: authorised by policy,
    /// well formed, from the caller's own feed, signed by the key its certificate
    /// binds, and not already fused (or `pending` in the same batch).
    fn admit(
        &self,
        peer: &WorkloadIdentity,
        body: &[u8],
        now: u64,
        pending: &BTreeSet<String>,
    ) -> Result<(VerifiedObservation, u64), Refusal> {
        let signed = decode_signed_observation(body);
        // A malformed submission is still an authenticated call, and is still audited.
        let action = if signed.is_ok() {
            ACTION_INGEST
        } else {
            "Ingest:MALFORMED"
        };
        let decision = lock(&self.governance)
            .authorize(&self.context(peer), &self.resource, action, now)
            .map_err(|_| Refusal::new(outcome::FAILED, "the observation could not be audited"))?;
        let signed = signed.map_err(|e| Refusal::new(outcome::MALFORMED, &e.to_string()))?;
        if !decision.permitted {
            return Err(Refusal::new(
                outcome::DENIED,
                "policy does not permit this identity to submit observations",
            ));
        }

        let feed = signed.observation.feed_id.0.clone();
        if feed != peer.id.0 {
            return Err(self.refuse(
                peer,
                now,
                "Ingest refused: observation from another feed",
                &feed,
                "a source submits only its own feed's observations",
            ));
        }
        let mut registry = FeedRegistry::new();
        registry.register(FeedId(peer.id.0.clone()), peer.verifying_key.clone());
        let verified = FeedGateway::new(registry, self.provider.clone())
            .authenticate(&signed)
            .map_err(|e| {
                self.refuse(
                    peer,
                    now,
                    "Ingest refused: feed signature rejected",
                    &feed,
                    &format!("the observation was rejected: {e}"),
                )
            })?;

        let observation_id = verified.observation().id.0.clone();
        if pending.contains(&observation_id) || lock(&self.state).already_fused(&observation_id) {
            return Err(self.refuse(
                peer,
                now,
                "Ingest refused: duplicate observation",
                &feed,
                &format!("observation {observation_id} was already fused"),
            ));
        }
        Ok((verified, decision.audit_seq))
    }

    /// A page of the picture as JSON: `{"layers": [..], "entities": [..], "next"}`.
    /// The body is empty (the first page) or `{"after"?: cursor|null, "layer"?: id}`.
    /// Entities come in id order, as many as fit in the page budget (at least one);
    /// when `next` is not null, the same query with `"after": next` returns the rest.
    /// Every page carries all the layers with their visibility. Each page is
    /// authorised as [`ACTION_READ_PICTURE`] and audited once; a malformed query is
    /// refused as malformed. [FR-H-1] [NFR-SEC-1]
    pub fn read_picture(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let query = parse_picture_query(body);
        let action = if query.is_ok() {
            ACTION_READ_PICTURE
        } else {
            "ReadPicture:MALFORMED"
        };
        let decision =
            lock(&self.governance).authorize(&self.context(peer), &self.resource, action, now);
        let (after, only_layer) = match (decision, query) {
            (Err(_), _) => {
                return self.reply(
                    READ_PICTURE,
                    outcome::FAILED,
                    "the read could not be audited",
                )
            }
            (Ok(_), Err(e)) => return self.reply(READ_PICTURE, outcome::MALFORMED, &e),
            (Ok(d), Ok(_)) if !d.permitted => {
                return self.reply(
                    READ_PICTURE,
                    outcome::DENIED,
                    "policy does not permit this identity to read the picture",
                )
            }
            (Ok(_), Ok(query)) => query,
        };
        let budget = self.picture_page_bytes.load(Ordering::Relaxed);

        let state = lock(&self.state);
        let picture = state.picture();
        let layers = picture
            .layers()
            .map(|layer| {
                Json::obj(vec![
                    ("id", Json::s(&layer.id.0)),
                    ("name", Json::s(&layer.name)),
                    ("visible", Json::Bool(picture.is_layer_visible(&layer.id))),
                ])
            })
            .collect();
        let mut bytes = 0usize;
        let mut entities = Vec::new();
        let mut next = Json::Null;
        let mut last: Option<&str> = None;
        let candidates = picture
            .entities()
            .filter(|e| after.as_deref().is_none_or(|a| e.id.0.as_str() > a))
            .filter(|e| only_layer.as_deref().is_none_or(|l| e.layer.0 == l));
        for entity in candidates {
            let json = entity_json(entity);
            let size = json.encode().len() + 1;
            if !entities.is_empty() && bytes + size > budget {
                next = last.map_or(Json::Null, Json::s);
                break;
            }
            bytes += size;
            last = Some(entity.id.0.as_str());
            entities.push(json);
        }
        let body = Json::obj(vec![
            ("layers", Json::Arr(layers)),
            ("entities", Json::Arr(entities)),
            ("next", next),
        ]);
        self.reply(READ_PICTURE, outcome::ACCEPTED, &body.encode())
    }

    /// A page of track history. The body is a JSON query: `{"from": T, "to": T,
    /// "layer"?: id, "entity"?: id, "step"?: seconds, "after"?: cursor}` (unix seconds,
    /// inclusive; `step` is the least spacing between returned points of a track). The
    /// response is `{"from","to","tracks":[{"id","kind","layer","points":[[t,lat,lon,
    /// alt_m|null],..]}],"next"}`, points by time; when `next` is not null, the same
    /// query with `"after": next` returns the rest. Each call is authorised as
    /// [`ACTION_READ_HISTORY`] and audited once; a malformed query, `to` before `from`
    /// or a window longer than the retention is refused as malformed. [FR-H-1]
    pub fn read_history(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let retention = self.history_retention.load(Ordering::Relaxed);
        let query = parse_history_query(body, retention);
        let action = if query.is_ok() {
            ACTION_READ_HISTORY
        } else {
            "ReadHistory:MALFORMED"
        };
        let decision =
            lock(&self.governance).authorize(&self.context(peer), &self.resource, action, now);
        let mut query = match (decision, query) {
            (Err(_), _) => {
                return self.reply(
                    READ_HISTORY,
                    outcome::FAILED,
                    "the read could not be audited",
                )
            }
            (Ok(_), Err(e)) => return self.reply(READ_HISTORY, outcome::MALFORMED, &e),
            (Ok(d), Ok(_)) if !d.permitted => {
                return self.reply(
                    READ_HISTORY,
                    outcome::DENIED,
                    "policy does not permit this identity to read track history",
                )
            }
            (Ok(_), Ok(query)) => query,
        };
        let (from, to) = (query.from, query.to);
        // Points older than the retention window are served by no read, pruned or not.
        query.from = query.from.max(now.saturating_sub(retention));
        query.max_bytes = self.history_page_bytes.load(Ordering::Relaxed);
        let state = lock(&self.state);
        let Some(history) = state.history() else {
            return self.reply(
                READ_HISTORY,
                outcome::FAILED,
                "this service keeps no track history",
            );
        };
        match history.page(&query) {
            Ok(page) => self.reply(READ_HISTORY, outcome::ACCEPTED, &page.to_json(from, to)),
            Err(e) => self.reply(READ_HISTORY, outcome::MALFORMED, &e.to_string()),
        }
    }

    /// The alerts, newest first: `{"state": "open"}` (not cleared, acknowledged or
    /// not) or `{"state": "all"}`; an empty body reads open ones. The response is
    /// `{"alerts": [alert, ..], "truncated": bool}` (see [`Alert::to_json`]), under
    /// [`ALERTS_PAGE_BYTES`]. Authorised as [`ACTION_READ_ALERTS`] and audited once; a
    /// malformed request is refused as malformed. [FR-H-4] [NFR-SEC-1]
    pub fn read_alerts(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let request = parse_alerts_request(body);
        let action = if request.is_ok() {
            ACTION_READ_ALERTS
        } else {
            "ReadAlerts:MALFORMED"
        };
        let decision =
            lock(&self.governance).authorize(&self.context(peer), &self.resource, action, now);
        let open_only = match (decision, request) {
            (Err(_), _) => {
                return self.reply(
                    READ_ALERTS,
                    outcome::FAILED,
                    "the read could not be audited",
                )
            }
            (Ok(_), Err(e)) => return self.reply(READ_ALERTS, outcome::MALFORMED, &e),
            (Ok(d), Ok(_)) if !d.permitted => {
                return self.reply(
                    READ_ALERTS,
                    outcome::DENIED,
                    "policy does not permit this identity to read alerts",
                )
            }
            (Ok(_), Ok(open_only)) => open_only,
        };
        let state = lock(&self.state);
        let mut bytes = 64;
        let mut truncated = false;
        let mut items = Vec::new();
        for alert in state.alerts().newest_first(open_only) {
            let json = alert.to_json();
            bytes += json.encode().len() + 1;
            if bytes > ALERTS_PAGE_BYTES {
                truncated = true;
                break;
            }
            items.push(json);
        }
        let body = Json::obj(vec![
            ("alerts", Json::Arr(items)),
            ("truncated", Json::Bool(truncated)),
        ]);
        self.reply(READ_ALERTS, outcome::ACCEPTED, &body.encode())
    }

    /// Acknowledge an alert: the body is `{"id": "A-1", "by": "<user name>"}` and the
    /// response the alert as it now is. Authorised as [`ACTION_ACKNOWLEDGE_ALERT`] and
    /// audited, then recorded as an `AcknowledgeAlert` event naming the alert and `by`;
    /// a detector's alert it clears is audited as `ClearAlert`. An unknown id is
    /// [`outcome::NOT_FOUND`]. [FR-H-4] [NFR-SEC-1]
    pub fn acknowledge_alert(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let request = parse_ack_request(body);
        let action = if request.is_ok() {
            ACTION_ACKNOWLEDGE_ALERT
        } else {
            "AcknowledgeAlert:MALFORMED"
        };
        let decision =
            lock(&self.governance).authorize(&self.context(peer), &self.resource, action, now);
        let (id, by) = match (decision, request) {
            (Err(_), _) => {
                return self.reply(
                    ACKNOWLEDGE_ALERT,
                    outcome::FAILED,
                    "the acknowledgement could not be audited",
                )
            }
            (Ok(_), Err(e)) => return self.reply(ACKNOWLEDGE_ALERT, outcome::MALFORMED, &e),
            (Ok(d), Ok(_)) if !d.permitted => {
                return self.reply(
                    ACKNOWLEDGE_ALERT,
                    outcome::DENIED,
                    "policy does not permit this identity to acknowledge alerts",
                )
            }
            (Ok(_), Ok(request)) => request,
        };
        let acknowledged = {
            let mut state = lock(&self.state);
            state
                .alerts_mut()
                .acknowledge(&id, &by, now)
                .map(|alert| (alert, state.alerts_mut().flush()))
        };
        let (alert, flushed) = match acknowledged {
            Ok(ok) => ok,
            Err(_) => {
                return self.reply(
                    ACKNOWLEDGE_ALERT,
                    outcome::NOT_FOUND,
                    &format!("there is no alert {id}"),
                )
            }
        };
        let recorded = lock(&self.governance).record_event(
            &peer.id.0,
            "AcknowledgeAlert",
            &format!("alert:{id} by:{by}"),
            now,
        );
        self.audit_alert_changes(now);
        if recorded.is_err() {
            return self.reply(
                ACKNOWLEDGE_ALERT,
                outcome::FAILED,
                "the acknowledgement could not be audited",
            );
        }
        if let Err(e) = flushed {
            return self.reply(
                ACKNOWLEDGE_ALERT,
                outcome::FAILED,
                &format!("the acknowledgement was not persisted: {e}"),
            );
        }
        self.reply(
            ACKNOWLEDGE_ALERT,
            outcome::ACCEPTED,
            &alert.to_json().encode(),
        )
    }

    fn refuse(
        &self,
        peer: &WorkloadIdentity,
        now: u64,
        action: &str,
        feed: &str,
        message: &str,
    ) -> Refusal {
        let recorded =
            lock(&self.governance).record_event(&peer.id.0, action, &format!("feed:{feed}"), now);
        let message = match recorded {
            Ok(_) => message.to_string(),
            Err(_) => format!("{message} (not audited)"),
        };
        Refusal::new(outcome::DENIED, &message)
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
            _ => "failed",
        };
        self.metrics.inc(
            "feed_requests_total",
            &[("method", method), ("outcome", label)],
        );
        let mut out = vec![code];
        out.extend_from_slice(message.as_bytes());
        out
    }
}

/// Why an observation was not admitted: the outcome byte and a message.
struct Refusal {
    code: u8,
    message: String,
}

impl Refusal {
    fn new(code: u8, message: &str) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }
}

/// The most observations one [`INGEST_BATCH`] request may carry.
pub const MAX_BATCH: usize = 10_000;

/// Frame encoded signed observations for [`INGEST_BATCH`]: a `u32` count, then each
/// observation as a `u32` length and its bytes, all little-endian.
pub fn encode_batch(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + items.iter().map(|i| 4 + i.len()).sum::<usize>());
    out.extend_from_slice(&(items.len() as u32).to_le_bytes());
    for item in items {
        out.extend_from_slice(&(item.len() as u32).to_le_bytes());
        out.extend_from_slice(item);
    }
    out
}

/// The inverse of [`encode_batch`]. A count over [`MAX_BATCH`], a truncated item and
/// trailing bytes are refused.
pub fn decode_batch(bytes: &[u8]) -> Result<Vec<&[u8]>, String> {
    let take_u32 = |at: usize| -> Result<usize, String> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| "the batch is truncated".to_string())
    };
    let n = take_u32(0)?;
    if n > MAX_BATCH {
        return Err(format!(
            "a batch carries at most {MAX_BATCH} observations, not {n}"
        ));
    }
    let mut pos = 4;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        let len = take_u32(pos)?;
        pos += 4;
        let item = bytes
            .get(pos..pos + len)
            .ok_or_else(|| "the batch is truncated".to_string())?;
        items.push(item);
        pos += len;
    }
    if pos != bytes.len() {
        return Err("the batch has trailing bytes".into());
    }
    Ok(items)
}

/// Parse a [`READ_HISTORY`] query, refusing `to` before `from` and a window longer
/// than `retention`. The page budget is the service's to set.
pub fn parse_history_query(body: &[u8], retention: u64) -> Result<HistoryQuery, String> {
    let text = std::str::from_utf8(body).map_err(|_| "the query is not UTF-8".to_string())?;
    let json =
        service_runtime::json::parse(text).map_err(|e| format!("the query is not JSON: {e}"))?;
    if !matches!(json, Json::Obj(_)) {
        return Err("the query is not a JSON object".into());
    }
    let seconds = |key: &str, required: bool| -> Result<Option<u64>, String> {
        match json.get(key) {
            None | Some(Json::Null) if !required => Ok(None),
            Some(Json::Int(n)) if *n >= 0 => Ok(Some(*n as u64)),
            _ => Err(format!("\"{key}\" must be a whole number of seconds")),
        }
    };
    let text_field = |key: &str| -> Result<Option<String>, String> {
        match json.get(key) {
            None | Some(Json::Null) => Ok(None),
            Some(Json::Str(s)) => Ok(Some(s.clone())),
            _ => Err(format!("\"{key}\" must be a string")),
        }
    };
    let from = seconds("from", true)?.unwrap_or(0);
    let to = seconds("to", true)?.unwrap_or(0);
    if to < from {
        return Err(format!("\"to\" ({to}) is before \"from\" ({from})"));
    }
    if to - from > retention {
        return Err(format!(
            "the window is {} s; history is kept for {retention} s",
            to - from
        ));
    }
    Ok(HistoryQuery {
        from,
        to,
        layer: text_field("layer")?,
        entity: text_field("entity")?,
        step: seconds("step", false)?.unwrap_or(0),
        after: text_field("after")?,
        max_bytes: HISTORY_PAGE_BYTES,
    })
}

/// Parse a [`READ_PICTURE`] query: the cursor after which to start, and a layer to
/// keep to. An empty body is the first page of every layer. [FR-H-1]
pub fn parse_picture_query(body: &[u8]) -> Result<(Option<String>, Option<String>), String> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok((None, None));
    }
    let json = request_object(body)?;
    if let Json::Obj(pairs) = &json {
        if let Some((key, _)) = pairs.iter().find(|(k, _)| k != "after" && k != "layer") {
            return Err(format!("unknown key \"{key}\""));
        }
    }
    let text = |key: &str| match json.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(s)) => Ok(Some(s.clone())),
        _ => Err(format!("\"{key}\" must be a string or null")),
    };
    Ok((text("after")?, text("layer")?))
}

/// Parse a [`READ_ALERTS`] request: whether only open alerts are wanted.
pub fn parse_alerts_request(body: &[u8]) -> Result<bool, String> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(true);
    }
    let json = request_object(body)?;
    if let Json::Obj(pairs) = &json {
        if let Some((key, _)) = pairs.iter().find(|(k, _)| k != "state") {
            return Err(format!("unknown key \"{key}\""));
        }
    }
    match json.get("state") {
        None => Ok(true),
        Some(Json::Str(s)) if s == "open" => Ok(true),
        Some(Json::Str(s)) if s == "all" => Ok(false),
        _ => Err("\"state\" must be \"open\" or \"all\"".into()),
    }
}

/// The most characters of an acknowledger's name recorded.
pub const MAX_ACK_BY: usize = 64;

/// Parse an [`ACKNOWLEDGE_ALERT`] request: the alert id and who acknowledges it (control
/// characters stripped, at most [`MAX_ACK_BY`] characters).
/// An [`ASK_PICTURE`] request.
#[derive(Debug, Clone, PartialEq)]
pub struct AskRequest {
    pub question: String,
    pub by: String,
    /// Areas, `[west, south, east, north]` in degrees.
    pub boxes: Vec<[f64; 4]>,
    /// The view's centre and radius: `(lat, lon, km)`.
    pub focus: Option<(f64, f64, f64)>,
}

/// Parse an [`ASK_PICTURE`] body.
pub fn parse_ask_request(body: &[u8]) -> Result<AskRequest, String> {
    let json = request_object(body)?;
    if let Json::Obj(pairs) = &json {
        if let Some((key, _)) = pairs
            .iter()
            .find(|(k, _)| !["question", "by", "boxes", "focus"].contains(&k.as_str()))
        {
            return Err(format!("unknown key \"{key}\""));
        }
    }
    let clean =
        |s: &str, n: usize| -> String { s.chars().filter(|c| !c.is_control()).take(n).collect() };
    let question = clean(json.str_field("question").unwrap_or(""), 400);
    if question.trim().len() < 3 {
        return Err("\"question\" must be a question".into());
    }
    let by = clean(json.str_field("by").unwrap_or(""), MAX_ACK_BY);
    if by.trim().is_empty() {
        return Err("\"by\" must name who asks".into());
    }
    let nums = |v: &Json| -> Option<Vec<f64>> {
        match v {
            Json::Arr(a) => a
                .iter()
                .map(|x| match x {
                    Json::Num(n) if n.is_finite() => Some(*n),
                    Json::Int(i) => Some(*i as f64),
                    _ => None,
                })
                .collect(),
            _ => None,
        }
    };
    let mut boxes = Vec::new();
    if let Some(Json::Arr(list)) = json.get("boxes") {
        for b in list.iter().take(32) {
            match nums(b).as_deref() {
                Some([w, s, e, n])
                    if (-90.0..=90.0).contains(s) && (-90.0..=90.0).contains(n) && s < n =>
                {
                    boxes.push([*w, *s, *e, *n])
                }
                _ => return Err("each box must be [west, south, east, north]".into()),
            }
        }
    }
    let focus = match json.get("focus") {
        None | Some(Json::Null) => None,
        Some(v) => match nums(v).as_deref() {
            Some([la, lo, km]) if (-90.0..=90.0).contains(la) && *km > 0.0 => Some((*la, *lo, *km)),
            _ => return Err("\"focus\" must be [lat, lon, km]".into()),
        },
    };
    Ok(AskRequest {
        question,
        by,
        boxes,
        focus,
    })
}

/// The evidence for a question: the area's most relevant entities and its summary.
fn ask_evidence(
    full: &Picture,
    alerts: &AlertBook,
    r: &AskRequest,
) -> (Vec<TrackedEntity>, String) {
    const STOP: &[&str] = &[
        "the", "and", "are", "any", "what", "which", "where", "how", "many", "there", "near",
        "with", "for", "from", "this", "that", "show", "tell", "about", "have", "has", "is", "in",
        "of", "on", "to", "a", "an", "me",
    ];
    let words: Vec<String> = r
        .question
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !STOP.contains(w))
        .map(|w| w.trim_end_matches('s').to_string())
        .collect();
    let pos = |e: &TrackedEntity| -> Option<(f64, f64)> {
        let n = |k: &str| match e.latest_attrs.get(k) {
            Some(Attr::Float(f)) if f.is_finite() => Some(*f),
            Some(Attr::Int(i)) => Some(*i as f64),
            _ => None,
        };
        Some((n("lat")?, n("lon")?))
    };
    let in_area = |e: &TrackedEntity| -> bool {
        let Some((la, lo)) = pos(e) else { return false };
        if let Some((fl, fo, km)) = r.focus {
            return alerts::distance_km(fl, fo, la, lo) <= km;
        }
        r.boxes.is_empty()
            || r.boxes.iter().any(|[w, s, e, n]| {
                la >= *s
                    && la <= *n
                    && if w <= e {
                        lo >= *w && lo <= *e
                    } else {
                        lo >= *w || lo <= *e
                    }
            })
    };
    let open: BTreeMap<&str, Vec<&Alert>> =
        alerts
            .alerts()
            .iter()
            .filter(|a| a.is_open())
            .fold(BTreeMap::new(), |mut m, a| {
                m.entry(a.entity.as_str()).or_default().push(a);
                m
            });
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut labels: BTreeMap<String, usize> = BTreeMap::new();
    let mut scored: Vec<(f64, &TrackedEntity)> = Vec::new();
    for e in full.entities().filter(|e| in_area(e)) {
        *kinds.entry(e.kind.as_str()).or_default() += 1;
        let alerted = open.get(e.id.0.as_str());
        if let Some(list) = alerted {
            for a in list {
                *labels
                    .entry(format!("{} ({})", a.label, a.severity.as_str()))
                    .or_default() += 1;
            }
        }
        let kind = e.kind.to_lowercase().replace('-', " ");
        let label = e
            .classification
            .as_ref()
            .map(|c| c.label.to_lowercase().replace('-', " "))
            .unwrap_or_default();
        let text: String = e
            .latest_attrs
            .values()
            .filter_map(|v| match v {
                Attr::Text(t) if t.len() <= 120 => Some(t.to_lowercase()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        let mut score = 0.0;
        for w in &words {
            if kind.contains(w.as_str()) || label.contains(w.as_str()) {
                score += 3.0;
            }
            if text.contains(w.as_str()) {
                score += 1.0;
            }
        }
        if alerted.is_some() {
            score += if score > 0.0 { 4.0 } else { 1.0 };
        }
        if e.kind.starts_with("infra-") || e.kind == "airspace-volume" {
            score *= 0.5;
        }
        if score > 0.0 {
            scored.push((score, e));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let evidence: Vec<TrackedEntity> = scored
        .into_iter()
        .take(ASK_MAX_ENTITIES)
        .map(|(_, e)| e.clone())
        .collect();
    let mut k: Vec<(&str, usize)> = kinds.into_iter().collect();
    k.sort_by(|a, b| b.1.cmp(&a.1));
    let mut l: Vec<(String, usize)> = labels.into_iter().collect();
    l.sort_by(|a, b| b.1.cmp(&a.1));
    let summary = format!(
        "entities in the area by kind: {}; open alerts in the area by label: {}",
        if k.is_empty() {
            "none".to_string()
        } else {
            k.iter()
                .take(20)
                .map(|(a, n)| format!("{a} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        },
        if l.is_empty() {
            "none".to_string()
        } else {
            l.iter()
                .take(15)
                .map(|(a, n)| format!("{a} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    (evidence, summary)
}

fn ask_json(a: &heimdall_decision_support::Answer, considered: usize, ms: u128) -> Json {
    let strs = |v: &[String]| Json::Arr(v.iter().map(|s| Json::s(s)).collect());
    Json::obj(vec![
        (
            "answer",
            a.answer.as_deref().map(Json::s).unwrap_or(Json::Null),
        ),
        ("citations", strs(&a.citations)),
        ("unresolved", strs(&a.unresolved)),
        ("grounded", Json::Bool(a.grounded())),
        ("inferences", strs(&a.inferences)),
        (
            "refusal",
            match &a.refusal {
                Some((code, detail)) => {
                    Json::obj(vec![("code", Json::s(code)), ("detail", Json::s(detail))])
                }
                None => Json::Null,
            },
        ),
        ("proposed", strs(&a.proposed)),
        ("considered", Json::Num(considered as f64)),
        ("advisory", Json::Bool(true)),
        ("ms", Json::Num(ms as f64)),
    ])
}

/// An [`ADVISE_OPTIONS`] request.
#[derive(Debug, Clone, PartialEq)]
pub struct AdviseRequest {
    pub alert: Option<String>,
    pub entity: Option<String>,
    pub question: Option<String>,
    pub by: String,
}

/// Parse an [`ADVISE_OPTIONS`] body: `{"alert"|"entity": id, "by": name, "question"?}`.
pub fn parse_advise_request(body: &[u8]) -> Result<AdviseRequest, String> {
    let json = request_object(body)?;
    if let Json::Obj(pairs) = &json {
        if let Some((key, _)) = pairs
            .iter()
            .find(|(k, _)| !["alert", "entity", "question", "by"].contains(&k.as_str()))
        {
            return Err(format!("unknown key \"{key}\""));
        }
    }
    let id = |k: &str| {
        json.str_field(k)
            .filter(|s| !s.trim().is_empty() && s.len() <= 128)
            .map(str::to_string)
    };
    let (alert, entity) = (id("alert"), id("entity"));
    if alert.is_none() == entity.is_none() {
        return Err("name exactly one of \"alert\" or \"entity\"".into());
    }
    let clean =
        |s: &str, n: usize| -> String { s.chars().filter(|c| !c.is_control()).take(n).collect() };
    let by = clean(json.str_field("by").unwrap_or(""), MAX_ACK_BY);
    if by.trim().is_empty() {
        return Err("\"by\" must name who asks".into());
    }
    let question = json
        .str_field("question")
        .map(|q| clean(q, 400))
        .filter(|q| !q.trim().is_empty());
    Ok(AdviseRequest {
        alert,
        entity,
        question,
        by,
    })
}

/// The decision-support picture for a request: the subject entity first, then its
/// nearest neighbours (within [`ADVISE_RADIUS_KM`], up to [`ADVISE_MAX_ENTITIES`] in
/// all), and the decision point in words. Returns (picture, decision point, subject id).
fn advise_picture(
    full: &Picture,
    alerts: &AlertBook,
    r: &AdviseRequest,
) -> Result<(Picture, String, String), String> {
    let alert = match &r.alert {
        Some(id) => Some(
            alerts
                .alerts()
                .iter()
                .find(|a| &a.id == id)
                .ok_or_else(|| format!("there is no alert {id}"))?,
        ),
        None => None,
    };
    let subject_id = alert
        .map(|a| a.entity.clone())
        .or_else(|| r.entity.clone())
        .unwrap_or_default();
    let subject = full
        .entity(&EntityId(subject_id.clone()))
        .ok_or_else(|| format!("{subject_id} is not in the picture"))?;
    let pos = |e: &TrackedEntity| -> Option<(f64, f64)> {
        let n = |k: &str| match e.latest_attrs.get(k) {
            Some(Attr::Float(f)) if f.is_finite() => Some(*f),
            Some(Attr::Int(i)) => Some(*i as f64),
            _ => None,
        };
        Some((n("lat")?, n("lon")?))
    };
    let mut picture = Picture::new();
    let add = |e: &TrackedEntity, picture: &mut Picture| {
        picture.ensure_layer(heimdall_picture::Layer::new(
            e.layer.0.clone(),
            e.layer.0.clone(),
        ));
        picture.upsert_entity(e.clone());
    };
    add(subject, &mut picture);
    if let Some((la, lo)) = pos(subject) {
        let mut near: Vec<(f64, &TrackedEntity)> = full
            .entities()
            .filter(|e| {
                e.id != subject.id && !e.kind.starts_with("infra-") && e.kind != "airspace-volume"
            })
            .filter_map(|e| pos(e).map(|(a, b)| (alerts::distance_km(la, lo, a, b), e)))
            .filter(|(d, _)| *d <= ADVISE_RADIUS_KM)
            .collect();
        near.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (_, e) in near.into_iter().take(ADVISE_MAX_ENTITIES - 1) {
            add(e, &mut picture);
        }
    }
    let mut point = match alert {
        Some(a) => format!(
            "Alert {} ({}) on entity:{}: {}. What should the operator do next?",
            a.label, a.severity.as_str(), a.entity, a.rationale
        ),
        None => format!("The operator has selected entity:{subject_id}. What, if anything, should they do about it?"),
    };
    if let Some(q) = &r.question {
        point.push_str(&format!(" The operator asks: {q}"));
    }
    Ok((picture, point, subject_id))
}

fn advise_json(
    a: &heimdall_decision_support::AdvisoryOptions,
    subject: &str,
    point: &str,
    considered: usize,
    ms: u128,
) -> Json {
    let options = a
        .option_set
        .options
        .iter()
        .map(|o| {
            let g = a.grounding_for(o.rank);
            Json::obj(vec![
                ("rank", Json::Num(o.rank as f64)),
                ("title", Json::s(&o.title)),
                ("summary", Json::s(&o.summary)),
                ("expected_effect", Json::s(&o.expected_effect)),
                (
                    "risks",
                    Json::Arr(o.risks.iter().map(|r| Json::s(r)).collect()),
                ),
                (
                    "citations",
                    Json::Arr(o.citations.iter().map(|c| Json::s(&c.raw)).collect()),
                ),
                ("grounded", Json::Bool(g.is_some_and(|g| g.grounded))),
                (
                    "unresolved",
                    Json::Arr(
                        g.map(|g| g.unresolved.iter().map(|c| Json::s(&c.raw)).collect())
                            .unwrap_or_default(),
                    ),
                ),
            ])
        })
        .collect();
    Json::obj(vec![
        ("subject", Json::s(subject)),
        ("decision_point", Json::s(point)),
        ("considered", Json::Num(considered as f64)),
        ("options", Json::Arr(options)),
        (
            "unsupported",
            a.option_set
                .unsupported
                .as_deref()
                .map(Json::s)
                .unwrap_or(Json::Null),
        ),
        ("advisory", Json::Bool(true)),
        ("ms", Json::Num(ms as f64)),
    ])
}

pub fn parse_ack_request(body: &[u8]) -> Result<(String, String), String> {
    let json = request_object(body)?;
    if let Json::Obj(pairs) = &json {
        if let Some((key, _)) = pairs.iter().find(|(k, _)| k != "id" && k != "by") {
            return Err(format!("unknown key \"{key}\""));
        }
    }
    let id = match json.str_field("id") {
        Some(id) if !id.trim().is_empty() && id.len() <= 128 => id.to_string(),
        _ => return Err("\"id\" must be an alert id".into()),
    };
    let by: String = json
        .str_field("by")
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_ACK_BY)
        .collect();
    if by.trim().is_empty() {
        return Err("\"by\" must name who acknowledges the alert".into());
    }
    Ok((id, by))
}

fn request_object(body: &[u8]) -> Result<Json, String> {
    let text = std::str::from_utf8(body).map_err(|_| "the request is not UTF-8".to_string())?;
    let json =
        service_runtime::json::parse(text).map_err(|e| format!("the request is not JSON: {e}"))?;
    if !matches!(json, Json::Obj(_)) {
        return Err("the request is not a JSON object".into());
    }
    Ok(json)
}

/// A lock that stays usable after a panic elsewhere.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn count(value: u64) -> Json {
    Json::Int(i64::try_from(value).unwrap_or(i64::MAX))
}

fn entity_json(entity: &TrackedEntity) -> Json {
    let classification = entity.classification.as_ref().map_or(Json::Null, |c| {
        Json::obj(vec![
            ("label", Json::s(&c.label)),
            (
                "source",
                Json::s(match c.source {
                    ClassificationSource::Rules => "rules",
                    ClassificationSource::Mimir => "mimir",
                }),
            ),
            ("rationale", Json::s(&c.rationale)),
        ])
    });
    let attributes = entity
        .latest_attrs
        .iter()
        .map(|(key, value)| {
            let value = match value {
                Attr::Text(t) => Json::Str(t.clone()),
                Attr::Int(i) => Json::Int(*i),
                Attr::Float(f) => Json::Num(*f),
                Attr::Bool(b) => Json::Bool(*b),
            };
            (key.clone(), value)
        })
        .collect();
    Json::obj(vec![
        ("id", Json::s(&entity.id.0)),
        ("kind", Json::s(&entity.kind)),
        ("layer", Json::s(&entity.layer.0)),
        (
            "sources",
            Json::Arr(entity.sources.iter().map(|s| Json::s(s)).collect()),
        ),
        ("firstSeen", count(entity.track.first_seen)),
        ("lastSeen", count(entity.track.last_seen)),
        ("observations", count(entity.track.observation_count)),
        ("classification", classification),
        ("attributes", Json::Obj(attributes)),
    ])
}

/// Parse `key=value,key=value` observation attributes: `true`/`false` are booleans, a
/// whole number an integer, another number a float, anything else text. A pair with
/// no `=` or an empty key is refused.
pub fn parse_attrs(text: &str) -> Result<Vec<(String, Attr)>, String> {
    text.split(',')
        .filter(|pair| !pair.trim().is_empty())
        .map(|pair| {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| format!("attribute '{pair}' is not key=value"))?;
            let key = key.trim();
            if key.is_empty() {
                return Err(format!("attribute '{pair}' has no key"));
            }
            let value = value.trim();
            let attr = match value {
                "true" => Attr::Bool(true),
                "false" => Attr::Bool(false),
                _ => match (value.parse::<i64>(), value.parse::<f64>()) {
                    (Ok(i), _) => Attr::Int(i),
                    (_, Ok(f)) => Attr::Float(f),
                    _ => Attr::Text(value.to_string()),
                },
            };
            Ok((key.to_string(), attr))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_are_typed_from_text() {
        assert_eq!(
            parse_attrs("speed=420, heading=87.5, callsign=ALPHA1, friendly=false").unwrap(),
            [
                ("speed".to_string(), Attr::Int(420)),
                ("heading".to_string(), Attr::Float(87.5)),
                ("callsign".to_string(), Attr::Text("ALPHA1".into())),
                ("friendly".to_string(), Attr::Bool(false)),
            ]
        );
        assert!(parse_attrs("").unwrap().is_empty());
        assert!(parse_attrs("speed").is_err());
        assert!(parse_attrs("=420").is_err());
    }

    #[test]
    fn a_history_query_is_parsed_and_a_bad_window_refused() {
        let q = parse_history_query(
            br#"{"from": 100, "to": 200, "layer": "air", "step": 5, "after": "150:AC-1"}"#,
            3600,
        )
        .unwrap();
        assert_eq!((q.from, q.to, q.step), (100, 200, 5));
        assert_eq!(q.layer.as_deref(), Some("air"));
        assert_eq!(q.entity, None);
        assert_eq!(q.after.as_deref(), Some("150:AC-1"));
        let q = parse_history_query(br#"{"from": 0, "to": 0, "entity": null}"#, 1).unwrap();
        assert_eq!((q.step, q.entity), (0, None));
        for bad in [
            &br#"{"to": 200}"#[..],
            br#"{"from": 300, "to": 200}"#,
            br#"{"from": 0, "to": 3601}"#,
            br#"{"from": -1, "to": 2}"#,
            br#"{"from": 1, "to": 2, "layer": 3}"#,
            br#"{"from": 1, "to": 2, "step": 1.5}"#,
            b"[]",
            b"not json",
        ] {
            assert!(
                parse_history_query(bad, 3600).is_err(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn a_question_names_who_asks_and_an_optional_area() {
        let q = parse_ask_request(br#"{"question":"Any power cuts near Leeds?","by":"analyst","boxes":[[-12,48,4,62]],"focus":[53.8,-1.55,40]}"#).unwrap();
        assert_eq!(q.boxes, vec![[-12.0, 48.0, 4.0, 62.0]]);
        assert_eq!(q.focus, Some((53.8, -1.55, 40.0)));
        assert!(
            parse_ask_request(br#"{"question":"What is open?","by":"a"}"#)
                .unwrap()
                .boxes
                .is_empty()
        );
        for bad in [
            &br#"{"question":"?","by":"a"}"#[..],
            br#"{"question":"What now?"}"#,
            br#"{"question":"What now?","by":"a","boxes":[[1,2,3]]}"#,
            br#"{"question":"What now?","by":"a","focus":[53,-1,0]}"#,
            br#"{"question":"What now?","by":"a","act":1}"#,
        ] {
            assert!(
                parse_ask_request(bad).is_err(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn an_advice_request_names_one_subject_and_who_asks() {
        let ok = parse_advise_request(br#"{"alert":"A-7","by":"analyst","question":"what next?"}"#)
            .unwrap();
        assert_eq!(
            (ok.alert.as_deref(), ok.entity, ok.by.as_str()),
            (Some("A-7"), None, "analyst")
        );
        assert_eq!(ok.question.as_deref(), Some("what next?"));
        assert!(parse_advise_request(br#"{"entity":"E-1","by":"a"}"#).is_ok());
        for bad in [
            &br#"{"alert":"A-1","entity":"E-1","by":"a"}"#[..],
            br#"{"by":"a"}"#,
            br#"{"alert":"A-1"}"#,
            br#"{"alert":"A-1","by":"a","act":"now"}"#,
        ] {
            assert!(
                parse_advise_request(bad).is_err(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn a_picture_query_is_parsed_and_a_bad_one_refused() {
        assert_eq!(parse_picture_query(b""), Ok((None, None)));
        assert_eq!(parse_picture_query(b" \n"), Ok((None, None)));
        assert_eq!(parse_picture_query(br#"{"after": null}"#), Ok((None, None)));
        assert_eq!(
            parse_picture_query(br#"{"after": "AC-1", "layer": "air"}"#),
            Ok((Some("AC-1".into()), Some("air".into())))
        );
        for bad in [
            &br#"{"after": 3}"#[..],
            br#"{"layer": true}"#,
            br#"{"from": 1}"#,
            b"[]",
            b"nope",
        ] {
            assert!(
                parse_picture_query(bad).is_err(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn alert_requests_are_parsed_and_bad_ones_refused() {
        assert_eq!(parse_alerts_request(b""), Ok(true));
        assert_eq!(parse_alerts_request(br#"{"state": "open"}"#), Ok(true));
        assert_eq!(parse_alerts_request(br#"{"state": "all"}"#), Ok(false));
        assert_eq!(parse_alerts_request(b"{}"), Ok(true));
        for bad in [
            &br#"{"state": "closed"}"#[..],
            br#"{"state": 1}"#,
            br#"{"state": "all", "x": 1}"#,
            b"[]",
            b"nope",
        ] {
            assert!(
                parse_alerts_request(bad).is_err(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
        assert_eq!(
            parse_ack_request(br#"{"id": "A-1", "by": "alice\u0007 smith"}"#),
            Ok(("A-1".to_string(), "alice smith".to_string()))
        );
        let long = format!(r#"{{"id": "A-1", "by": "{}"}}"#, "x".repeat(100));
        assert_eq!(
            parse_ack_request(long.as_bytes()).unwrap().1.len(),
            MAX_ACK_BY
        );
        for bad in [
            &br#"{"id": "A-1"}"#[..],
            br#"{"id": "A-1", "by": " "}"#,
            br#"{"by": "alice"}"#,
            br#"{"id": "", "by": "alice"}"#,
            br#"{"id": 1, "by": "alice"}"#,
            br#"{"id": "A-1", "by": "alice", "note": "x"}"#,
            b"",
        ] {
            assert!(
                parse_ack_request(bad).is_err(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn a_batch_round_trips_and_a_malformed_one_is_refused() {
        let items = vec![b"one".to_vec(), Vec::new(), b"three".to_vec()];
        let bytes = encode_batch(&items);
        let decoded = decode_batch(&bytes).unwrap();
        assert_eq!(decoded, [&b"one"[..], &b""[..], &b"three"[..]]);
        assert!(
            decode_batch(&bytes[..bytes.len() - 1]).is_err(),
            "truncated"
        );
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_batch(&trailing).is_err(), "trailing bytes");
        assert!(
            decode_batch(&((MAX_BATCH as u32 + 1).to_le_bytes())).is_err(),
            "too many"
        );
        assert!(decode_batch(&[]).is_err(), "empty");
        assert_eq!(decode_batch(&encode_batch(&[])).unwrap().len(), 0);
    }
}
