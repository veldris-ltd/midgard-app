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
//!    feed gateway's admission check — the only way to a `VerifiedObservation`;
//! 5. its `observed_at` lies inside the accepted window around the time the service
//!    received it ([`IngestLimits`]), and its observation id has not already been
//!    received from that feed. A feed's clock is its own assertion: without the
//!    window one certified feed could date an observation years ahead and freeze any
//!    entity, and without the replay check resubmitting it would inflate the count.
//!    [VLD-ANB-MIDGARD-0015]
//!
//! A refusal at 3, 4 or 5 is audited too, with a reason code and the entity key.
//! Admitted observations are fused and each changed entity is appended to the sealed
//! picture journal, so a restart recovers it. [RC-13] The picture records when each entity was last
//! received, and [`READ_PICTURE`] says which entities have gone stale.
//!
//! The picture is bounded by [`PictureLimits`]: an observation with too many or too
//! large attributes is malformed, and a new entity that would pass the entity cap,
//! overall or for its feed, first evicts tracks quiet for longer than `evict_after`
//! and is otherwise refused as `picture-full` and audited. Ingest is rate limited per
//! identity by the gateway's limiter, which the binary turns on. A picture read is
//! paged and never answers with more than [`MAX_PAGE_BODY`] bytes, so the analyst
//! client's frame cap is never reached. [VLD-ANB-MIDGARD-0017]
//!
//! Responses are one outcome byte ([`outcome`]) followed by UTF-8: a message, or for
//! [`READ_PICTURE`] a page of the picture as JSON.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use audit_writer::{LedgerStore, SealedEntry};
use gateway::{RequestHandler, Router};
use governance::{AuditContext, GovContext, Governance, Outcome};
use heimdall_feed_gateway::{
    decode_signed_observation, FeedGateway, FeedRegistry, VerifiedObservation,
};
use heimdall_fusion::FusionEngine;
use heimdall_picture::{
    Attr, ClassificationSource, EntityId, FeedId, LayerId, ObservationId, Picture, PictureDelta,
    PictureLimits, TrackedEntity,
};
use heimdall_store::{DurablePicture, StoreError};
use policy::{ClassificationLevel, Clearance, Purpose, Resource};
use pq_mtls::WorkloadIdentity;
use pqc_agility::SharedProvider;
use service_runtime::json::{self, Json};
use telemetry::MetricsRegistry;

/// The method a source submits one encoded signed observation with.
pub const INGEST: &str = "Ingest";
/// The method an analyst reads the picture with. Its payload is empty for the first
/// page, or a JSON object `{"offset": n, "limit": m, "includeHidden": bool}`, every
/// field optional.
pub const READ_PICTURE: &str = "ReadPicture";
/// The policy action a submission is authorised as.
pub const ACTION_INGEST: &str = "Ingest";
/// The policy action a picture read is authorised as.
pub const ACTION_READ_PICTURE: &str = "ReadPicture";

/// Entities a picture page holds when the request does not say.
pub const DEFAULT_PAGE_LIMIT: usize = 500;
/// Entities a picture page holds at most, whatever the request says.
pub const MAX_PAGE_LIMIT: usize = 2_000;
/// The most bytes a picture page's body may take: well under the mesh record frame
/// cap (`pq_mtls::MAX_RECORD_FRAME`, 1 MiB) once the record is sealed and framed.
pub const MAX_PAGE_BODY: usize = 900 * 1024;

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
}

/// What fusing an observation did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FuseOutcome {
    /// Fused into `entity`, after evicting `evicted` stale entities to make room.
    Fused {
        /// The entity the observation updated or opened.
        entity: EntityId,
        /// Stale entities evicted to make room for it.
        evicted: usize,
    },
    /// Refused: the observation would open a new entity past a cap, and evicting
    /// `evicted` stale entities did not make room.
    Full {
        /// Stale entities evicted before it was refused.
        evicted: usize,
    },
}


/// The reason code a refusal for an `observed_at` outside the window is audited with.
pub const REASON_OUT_OF_WINDOW: &str = "observed-at-out-of-window";
/// The reason code a refusal for a repeated observation id is audited with.
pub const REASON_REPLAY: &str = "replay";
/// How many recent observation ids are remembered per feed for replay refusal.
pub const REPLAY_RING_CAPACITY: usize = 4096;

/// How far a feed's `observed_at` may stray from the time the service receives the
/// observation, and when an entity no longer received counts as stale. All in
/// seconds. [VLD-ANB-MIDGARD-0015]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestLimits {
    /// How far ahead of receipt `observed_at` may be (feed clock drift).
    pub max_future_skew: u64,
    /// How far behind receipt `observed_at` may be (delayed or backfilled feeds).
    pub max_age: u64,
    /// How long after its last receipt an entity is reported as stale.
    pub stale_after: u64,
}

impl Default for IngestLimits {
    /// Five minutes ahead, seven days behind, stale after fifteen minutes.
    fn default() -> Self {
        Self {
            max_future_skew: 300,
            max_age: 604_800,
            stale_after: 900,
        }
    }
}

impl IngestLimits {
    /// Whether an observation the feed dates `observed_at` may be fused when it is
    /// received at `received_at`: no more than `max_future_skew` ahead and no more
    /// than `max_age` behind.
    pub fn admits(&self, observed_at: u64, received_at: u64) -> bool {
        observed_at <= received_at.saturating_add(self.max_future_skew)
            && observed_at.saturating_add(self.max_age) >= received_at
    }

    /// Whether an entity last received at `last_received` is stale at `now`.
    pub fn is_stale(&self, last_received: u64, now: u64) -> bool {
        now.saturating_sub(last_received) > self.stale_after
    }
}

/// The observation ids most recently received from one feed: the order they came
/// in, to forget the oldest, and a set to look them up in. Memory only, so a restart
/// admits one more copy of each, which the window bounds. [VLD-ANB-MIDGARD-0015]
#[derive(Default)]
struct RecentIds {
    order: VecDeque<ObservationId>,
    seen: BTreeSet<ObservationId>,
}

impl RecentIds {
    fn contains(&self, id: &ObservationId) -> bool {
        self.seen.contains(id)
    }

    fn insert(&mut self, id: ObservationId) {
        if !self.seen.insert(id.clone()) {
            return;
        }
        self.order.push_back(id);
        if self.order.len() > REPLAY_RING_CAPACITY {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
    }
}

/// The fused picture and its sealed, durable copy, kept in step, within its limits,
/// and the observation ids recently received from each feed.
pub struct PictureState {
    engine: FusionEngine,
    durable: DurablePicture,
    limits: PictureLimits,
    /// Entities each feed has contributed to, kept in step with the picture.
    per_feed: BTreeMap<String, usize>,
    recent_ids: BTreeMap<String, RecentIds>,
}

impl PictureState {
    /// Resume fusion over a recovered picture: the engine starts from everything the
    /// durable picture holds, so later observations extend the recovered tracks. The
    /// limits are [`PictureLimits::default`] until [`PictureState::with_limits`].
    pub fn recover(durable: DurablePicture) -> Self {
        let mut engine = FusionEngine::new();
        PictureDelta::snapshot(durable.picture()).merge_into(engine.picture_mut());
        for layer in durable.picture().layers() {
            if !durable.picture().is_layer_visible(&layer.id) {
                engine.picture_mut().set_layer_visible(&layer.id, false);
            }
        }
        let per_feed = engine.picture().entities_by_feed_count();
        Self {
            engine,
            durable,
            limits: PictureLimits::default(),
            per_feed,
            recent_ids: BTreeMap::new(),
        }
    }

    /// The same state held to `limits`.
    pub fn with_limits(mut self, limits: PictureLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Hold the picture to `limits` from now on. Entities already held past a lower
    /// cap stay until they are evicted.
    pub fn set_limits(&mut self, limits: PictureLimits) {
        self.limits = limits;
    }

    /// The limits the picture is held to.
    pub fn limits(&self) -> PictureLimits {
        self.limits
    }

    /// The current picture.
    pub fn picture(&self) -> &Picture {
        self.engine.picture()
    }

    /// Show or hide a layer, durably: entities on a hidden layer are left out of a
    /// picture read unless it asks for them.
    pub fn set_layer_visible(&mut self, layer: &LayerId, visible: bool) -> Result<(), StoreError> {
        self.durable.set_layer_visible(layer, visible)?;
        self.engine.picture_mut().set_layer_visible(layer, visible);
        Ok(())
    }

    /// Whether this observation's id has already been received from its feed (within
    /// the last [`REPLAY_RING_CAPACITY`] from that feed). [VLD-ANB-MIDGARD-0015]
    pub fn is_replay(&self, observation: &VerifiedObservation) -> bool {
        let o = observation.observation();
        self.recent_ids
            .get(&o.feed_id.0)
            .is_some_and(|ids| ids.contains(&o.id))
    }

    /// Fuse a verified observation received at `now` and journal the entity it
    /// changed. An observation that would open a new entity past the overall or its
    /// feed's cap first evicts every entity not seen for `evict_after`, removing each
    /// from the journal too; if that makes no room it is [`FuseOutcome::Full`] and
    /// the observation is not fused. [VLD-ANB-MIDGARD-0017] A fused observation's id is
    /// remembered for its feed at once, so a resubmission is a replay even if sealing
    /// failed. [VLD-ANB-MIDGARD-0015]
    pub fn fuse(
        &mut self,
        observation: &VerifiedObservation,
        now: u64,
    ) -> Result<FuseOutcome, StoreError> {
        let o = observation.observation();
        let before = self
            .engine
            .picture()
            .entity(&EntityId(o.entity_key.clone()))
            .map(|e| e.sources.clone());
        let mut evicted = 0;
        if before.is_none() && self.at_cap(&o.feed_id.0) {
            evicted = self.evict_stale(now)?;
            if self.at_cap(&o.feed_id.0) {
                return Ok(FuseOutcome::Full { evicted });
            }
        }

        let id = self.engine.ingest(observation, now);
        self.recent_ids
            .entry(o.feed_id.0.clone())
            .or_default()
            .insert(o.id.clone());
        let Some(entity) = self.engine.picture().entity(&id).cloned() else {
            return Ok(FuseOutcome::Fused {
                entity: id,
                evicted,
            });
        };
        for feed in entity.sources.difference(&before.unwrap_or_default()) {
            *self.per_feed.entry(feed.clone()).or_insert(0) += 1;
        }
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
        self.durable.upsert_entity(entity)?;
        Ok(FuseOutcome::Fused {
            entity: id,
            evicted,
        })
    }

    /// Whether a new entity from `feed` would pass a cap.
    fn at_cap(&self, feed: &str) -> bool {
        self.engine.picture().entity_count() >= self.limits.max_entities
            || self.per_feed.get(feed).copied().unwrap_or(0) >= self.limits.max_entities_per_feed
    }

    /// Evict every stale entity from the engine and the journal; how many went.
    fn evict_stale(&mut self, now: u64) -> Result<usize, StoreError> {
        let stale = self.engine.evict_stale(now, self.limits.evict_after);
        for id in &stale {
            if let Some(entity) = self.durable.remove_entity(id)? {
                self.forget_sources(&entity.sources);
            }
        }
        Ok(stale.len())
    }

    fn forget_sources(&mut self, sources: &BTreeSet<String>) {
        for feed in sources {
            if let Some(n) = self.per_feed.get_mut(feed) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    self.per_feed.remove(feed);
                }
            }
        }
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
    limits: IngestLimits,
}

impl<S: LedgerStore + Send + 'static> FeedService<S> {
    /// Assemble the service. `resource` is what requests are authorised against and
    /// `purpose` the purpose they are audited under; `provider` verifies feed
    /// signatures; `clock` gives the receipt time observations are bounded against
    /// under `limits`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        state: PictureState,
        governance: Governance<S>,
        provider: SharedProvider,
        resource: Resource,
        purpose: Purpose,
        clock: Arc<dyn Fn() -> u64 + Send + Sync>,
        metrics: Arc<MetricsRegistry>,
        limits: IngestLimits,
    ) -> Arc<Self> {
        metrics.describe(
            "feed_requests_total",
            "Feed requests, by method and outcome.",
        );
        metrics.describe("picture_entities", "Entities the picture holds.");
        metrics.describe(
            "picture_evictions_total",
            "Stale entities evicted to keep the picture within its caps.",
        );
        metrics.set_gauge(
            "picture_entities",
            &[],
            state.picture().entity_count() as f64,
        );
        Arc::new(Self {
            state: Mutex::new(state),
            governance: Mutex::new(governance),
            provider,
            resource,
            purpose,
            clock,
            metrics,
            limits,
        })
    }

    /// The audit ledger's sealed entries, oldest first (for verification and tests).
    pub fn audit_entries(&self) -> Vec<SealedEntry> {
        lock(&self.governance).audit_entries().to_vec()
    }

    /// The router the PQ-mTLS gateway serves: [`INGEST`] and [`READ_PICTURE`].
    pub fn router(self: &Arc<Self>) -> Router {
        let service = self.clone();
        let ingest: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.ingest(peer, body));
        let service = self.clone();
        let read: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.read_picture(peer, body));
        Router::new()
            .route(INGEST, ingest)
            .route(READ_PICTURE, read)
    }

    /// Admit, fuse and persist one signed observation.
    pub fn ingest(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let signed = decode_signed_observation(body);
        // A malformed submission is still an authenticated call, and is still audited.
        let action = if signed.is_ok() {
            ACTION_INGEST
        } else {
            "Ingest:MALFORMED"
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
                    INGEST,
                    outcome::FAILED,
                    "the observation could not be audited",
                )
            }
        };
        let signed = match signed {
            Ok(signed) => signed,
            Err(e) => return self.reply(INGEST, outcome::MALFORMED, &e.to_string()),
        };
        if !decision.permitted {
            return self.reply(
                INGEST,
                outcome::DENIED,
                "policy does not permit this identity to submit observations",
            );
        }

        let feed = signed.observation.feed_id.0.clone();
        let entity_key = signed.observation.entity_key.clone();
        if feed != peer.id.0 {
            return self.refuse(
                peer,
                now,
                "Ingest refused: observation from another feed",
                &feed,
                "a source submits only its own feed's observations",
                refusal("another-feed", &entity_key),
            );
        }
        let mut registry = FeedRegistry::new();
        registry.register(FeedId(peer.id.0.clone()), peer.verifying_key.clone());
        let verified = match FeedGateway::new(registry, self.provider.clone()).authenticate(&signed)
        {
            Ok(verified) => verified,
            Err(e) => {
                return self.refuse(
                    peer,
                    now,
                    "Ingest refused: feed signature rejected",
                    &feed,
                    &format!("the observation was rejected: {e}"),
                    refusal("signature", &entity_key),
                )
            }
        };

        let limits = lock(&self.state).limits();
        if let Err(e) = limits.check_observation(verified.observation()) {
            return self.reply(INGEST, outcome::MALFORMED, &e);
        }

        // The service's own clock is the receipt time; the feed's `observed_at` is
        // only accepted near it. [VLD-ANB-MIDGARD-0015]
        let received_at = now;
        if !self
            .limits
            .admits(verified.observation().observed_at, received_at)
        {
            return self.refuse(
                peer,
                now,
                "Ingest refused: observed_at outside the accepted window",
                &feed,
                "the observation time is outside the accepted window",
                refusal(REASON_OUT_OF_WINDOW, &entity_key),
            );
        }

        // The replay check and the fusion that records the id happen under one lock,
        // so two concurrent copies cannot both be fused.
        let mut state = lock(&self.state);
        if state.is_replay(&verified) {
            drop(state);
            return self.refuse(
                peer,
                now,
                "Ingest refused: replayed observation",
                &feed,
                "this observation has already been received from this feed",
                refusal(REASON_REPLAY, &entity_key),
            );
        }
        let fused = state.fuse(&verified, received_at);
        let entities = state.picture().entity_count();
        drop(state);
        self.metrics
            .set_gauge("picture_entities", &[], entities as f64);
        match fused {
            Ok(FuseOutcome::Fused { entity, evicted }) => {
                self.count_evictions(evicted);
                self.reply(
                    INGEST,
                    outcome::ACCEPTED,
                    &format!(
                        "fused into entity {} at audit sequence {}",
                        entity.0, decision.audit_seq
                    ),
                )
            }
            Ok(FuseOutcome::Full { evicted }) => {
                self.count_evictions(evicted);
                self.refuse(
                    peer,
                    now,
                    "Ingest refused: picture full",
                    &feed,
                    "picture-full: the picture holds as many entities as it may and none \
                     is stale enough to evict",
                    refusal("picture-full", &entity_key),
                )
            }
            Err(e) => self.reply(
                INGEST,
                outcome::FAILED,
                &format!("the picture was not persisted: {e:?}"),
            ),
        }
    }

    /// Hold the picture to `limits` from now on.
    pub fn set_picture_limits(&self, limits: PictureLimits) {
        lock(&self.state).set_limits(limits);
    }

    /// Show or hide a layer of the picture, durably.
    pub fn set_layer_visible(&self, layer: &LayerId, visible: bool) -> Result<(), StoreError> {
        lock(&self.state).set_layer_visible(layer, visible)
    }

    /// The action of every audit entry so far, oldest first.
    pub fn audited_actions(&self) -> Vec<String> {
        lock(&self.governance)
            .audit_entries()
            .iter()
            .map(|e| e.record.action.0.clone())
            .collect()
    }

    fn count_evictions(&self, evicted: usize) {
        if evicted > 0 {
            self.metrics
                .inc_by("picture_evictions_total", &[], evicted as u64);
        }
    }

    /// A page of the picture as JSON: layers with visibility, and the tracked
    /// entities from `offset` in id order, at most `limit` of them (default
    /// [`DEFAULT_PAGE_LIMIT`], at most [`MAX_PAGE_LIMIT`]), leaving out entities on
    /// hidden layers unless `includeHidden`. The limit is halved until the body fits
    /// in [`MAX_PAGE_BODY`]; the page says `total`, `offset`, the `limit` it used and
    /// whether it is `truncated` (more follow). [VLD-ANB-MIDGARD-0017]
    pub fn read_picture(&self, peer: &WorkloadIdentity, request: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        match lock(&self.governance).authorize(
            &self.context(peer),
            &self.resource,
            ACTION_READ_PICTURE,
            now,
        ) {
            Ok(d) if d.permitted => {}
            Ok(_) => {
                return self.reply(
                    READ_PICTURE,
                    outcome::DENIED,
                    "policy does not permit this identity to read the picture",
                )
            }
            Err(_) => {
                return self.reply(
                    READ_PICTURE,
                    outcome::FAILED,
                    "the read could not be audited",
                )
            }
        }

        let page = match PageRequest::parse(request) {
            Ok(page) => page,
            Err(e) => return self.reply(READ_PICTURE, outcome::MALFORMED, &e),
        };
        let body = picture_page(lock(&self.state).picture(), &page, now, &self.limits);
        self.reply(READ_PICTURE, outcome::ACCEPTED, &body)
    }

    fn refuse(
        &self,
        peer: &WorkloadIdentity,
        now: u64,
        action: &str,
        feed: &str,
        message: &str,
        audit: AuditContext,
    ) -> Vec<u8> {
        let recorded = lock(&self.governance).record_event_with(
            &peer.id.0,
            action,
            &format!("feed:{feed}"),
            now,
            audit,
        );
        let message = match recorded {
            Ok(_) => message.to_string(),
            Err(_) => format!("{message} (not audited)"),
        };
        self.reply(INGEST, outcome::DENIED, &message)
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
            "feed_requests_total",
            &[("method", method), ("outcome", label)],
        );
        let mut out = vec![code];
        out.extend_from_slice(message.as_bytes());
        out
    }
}

/// A lock that stays usable after a panic elsewhere.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The audit context of an ingest refusal: outcome, reason code and the entity key
/// the observation named. [VLD-ANB-MIDGARD-0015]
fn refusal(reason: &str, entity_key: &str) -> AuditContext {
    AuditContext {
        outcome: Outcome::Refused,
        reason: Some(reason.to_string()),
        target: Some(
            entity_key
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect(),
        ),
        ..AuditContext::default()
    }
}

fn count(value: u64) -> Json {
    Json::Int(i64::try_from(value).unwrap_or(i64::MAX))
}

fn entity_json(
    entity: &TrackedEntity,
    omit_attributes: bool,
    now: u64,
    limits: &IngestLimits,
) -> Json {
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
        .filter(|_| !omit_attributes)
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
    let mut fields = vec![
        ("id", Json::s(&entity.id.0)),
        ("kind", Json::s(&entity.kind)),
        ("layer", Json::s(&entity.layer.0)),
        (
            "sources",
            Json::Arr(entity.sources.iter().map(|s| Json::s(s)).collect()),
        ),
        ("firstSeen", count(entity.track.first_seen)),
        ("lastSeen", count(entity.track.last_seen)),
        ("lastReceived", count(entity.track.last_received)),
        (
            "stale",
            Json::Bool(limits.is_stale(entity.track.last_received, now)),
        ),
        ("observations", count(entity.track.observation_count)),
        ("classification", classification),
        ("attributes", Json::Obj(attributes)),
    ];
    if omit_attributes {
        fields.push(("attributesOmitted", Json::Bool(true)));
    }
    Json::obj(fields)
}

/// What a [`READ_PICTURE`] request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    /// Entities to skip, in id order.
    pub offset: usize,
    /// Entities to return at most, within `1..=MAX_PAGE_LIMIT`.
    pub limit: usize,
    /// Whether entities on hidden layers are included.
    pub include_hidden: bool,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DEFAULT_PAGE_LIMIT,
            include_hidden: false,
        }
    }
}

impl PageRequest {
    /// Parse a request body: empty is the first page with the defaults; otherwise a
    /// JSON object whose `offset`, `limit` and `includeHidden` are each optional. A
    /// limit is clamped to `1..=MAX_PAGE_LIMIT`; a negative or mistyped field is an
    /// error.
    pub fn parse(body: &[u8]) -> Result<Self, String> {
        let mut page = Self::default();
        if body.iter().all(u8::is_ascii_whitespace) {
            return Ok(page);
        }
        let text = std::str::from_utf8(body).map_err(|_| "the page request is not UTF-8")?;
        let request = json::parse(text).map_err(|e| format!("the page request: {e}"))?;
        if !matches!(request, Json::Obj(_)) {
            return Err("the page request is not a JSON object".into());
        }
        let whole = |key: &str| -> Result<Option<usize>, String> {
            match request.get(key) {
                None | Some(Json::Null) => Ok(None),
                Some(v) => v
                    .as_i64()
                    .and_then(|n| usize::try_from(n).ok())
                    .map(Some)
                    .ok_or_else(|| format!("'{key}' must be a whole number")),
            }
        };
        if let Some(offset) = whole("offset")? {
            page.offset = offset;
        }
        if let Some(limit) = whole("limit")? {
            page.limit = limit.clamp(1, MAX_PAGE_LIMIT);
        }
        match request.get("includeHidden") {
            None | Some(Json::Null) => {}
            Some(Json::Bool(b)) => page.include_hidden = *b,
            Some(_) => return Err("'includeHidden' must be true or false".into()),
        }
        Ok(page)
    }

    /// The request as a [`READ_PICTURE`] payload.
    pub fn encode(&self) -> Vec<u8> {
        Json::obj(vec![
            ("offset", count(self.offset as u64)),
            ("limit", count(self.limit as u64)),
            ("includeHidden", Json::Bool(self.include_hidden)),
        ])
        .encode()
        .into_bytes()
    }
}

/// The JSON body of one picture page, at most [`MAX_PAGE_BODY`] bytes: the limit is
/// halved until it fits. An entity too large to fit on a page by itself is sent
/// with its attributes left out and `attributesOmitted` set.
fn picture_page(picture: &Picture, page: &PageRequest, now: u64, limits: &IngestLimits) -> String {
    let layers: Vec<Json> = picture
        .layers()
        .map(|layer| {
            Json::obj(vec![
                ("id", Json::s(&layer.id.0)),
                ("name", Json::s(&layer.name)),
                ("visible", Json::Bool(picture.is_layer_visible(&layer.id))),
            ])
        })
        .collect();
    let shown = |e: &&TrackedEntity| page.include_hidden || picture.is_layer_visible(&e.layer);
    let total = picture.entities().filter(shown).count();
    let encode = |limit: usize, omit_attributes: bool| {
        let entities: Vec<Json> = picture
            .entities()
            .filter(shown)
            .skip(page.offset)
            .take(limit)
            .map(|e| entity_json(e, omit_attributes, now, limits))
            .collect();
        let returned = entities.len();
        Json::obj(vec![
            ("layers", Json::Arr(layers.clone())),
            ("entities", Json::Arr(entities)),
            ("total", count(total as u64)),
            ("offset", count(page.offset as u64)),
            ("limit", count(limit as u64)),
            (
                "truncated",
                Json::Bool(page.offset.saturating_add(returned) < total),
            ),
        ])
        .encode()
    };
    let mut limit = page.limit;
    let mut body = encode(limit, false);
    while body.len() > MAX_PAGE_BODY && limit > 1 {
        limit /= 2;
        body = encode(limit, false);
    }
    if body.len() > MAX_PAGE_BODY {
        body = encode(1, true);
    }
    body
}

/// Parse `key=value,key=value` observation attributes: `true`/`false` are booleans, a
/// whole number an integer, another finite number a float, anything else text. Rust
/// reads `nan`, `inf` and `infinity` as floats, but no JSON can carry them, so they stay
/// text: the airport code `NAN` (Nadi) must not become a missing number. A pair with no
/// `=` or an empty key is refused. [FR-H-3]
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
                    (_, Ok(f)) if f.is_finite() => Attr::Float(f),
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
    fn words_rust_reads_as_non_finite_floats_stay_text() {
        assert_eq!(
            parse_attrs("iata=NAN, a=inf, b=-Infinity, c=nan, d=1e3").unwrap(),
            [
                ("iata".to_string(), Attr::Text("NAN".into())),
                ("a".to_string(), Attr::Text("inf".into())),
                ("b".to_string(), Attr::Text("-Infinity".into())),
                ("c".to_string(), Attr::Text("nan".into())),
                ("d".to_string(), Attr::Float(1000.0)),
            ]
        );
    }
}
