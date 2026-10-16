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
//! Responses are one outcome byte ([`outcome`]) followed by UTF-8: a message, or for
//! [`READ_PICTURE`] the picture as JSON.

use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
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
use heimdall_store::{DurablePicture, StoreError};
use policy::{ClassificationLevel, Clearance, Purpose, Resource};
use pq_mtls::WorkloadIdentity;
use pqc_agility::SharedProvider;
use service_runtime::json::Json;
use telemetry::MetricsRegistry;

pub mod rules_file;
pub use rules_file::parse_rules;

/// The most often stale tracks are swept, in seconds of the service clock.
pub const EXPIRY_SWEEP_SECS: u64 = 30;
/// How many recent observation ids are remembered to refuse replays. Older ids fall
/// out first; ids are also forgotten on restart, which the ledger records.
pub const REPLAY_MEMORY: usize = 200_000;

/// The method a source submits one encoded signed observation with.
pub const INGEST: &str = "Ingest";
/// The method a source submits many encoded signed observations with, framed by
/// [`encode_batch`].
pub const INGEST_BATCH: &str = "IngestBatch";
/// The method an analyst reads the picture with; its payload is empty.
pub const READ_PICTURE: &str = "ReadPicture";
/// The policy action a submission is authorised as.
pub const ACTION_INGEST: &str = "Ingest";
/// The policy action a picture read is authorised as.
pub const ACTION_READ_PICTURE: &str = "ReadPicture";

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

/// The fused picture and its sealed, durable copy, kept in step.
pub struct PictureState {
    engine: FusionEngine,
    durable: DurablePicture,
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
            seen: BTreeSet::new(),
            seen_order: VecDeque::new(),
        }
    }

    /// Whether an observation with this id has already been fused (since start).
    pub fn already_fused(&self, observation_id: &str) -> bool {
        self.seen.contains(observation_id)
    }

    /// Remove entities last seen before `cutoff`, from the live and the sealed picture.
    pub fn expire_before(&mut self, cutoff: u64) -> Result<Vec<EntityId>, StoreError> {
        self.engine.expire_before(cutoff);
        self.durable.expire_before(cutoff)
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

    /// Fuse verified observations in order and seal the picture once for all of them.
    pub fn fuse_many(&mut self, observations: &[VerifiedObservation]) -> Result<(), StoreError> {
        if observations.is_empty() {
            return Ok(());
        }
        let mut changed = BTreeSet::new();
        for observation in observations {
            changed.insert(self.engine.ingest(observation));
        }
        let layers: Vec<_> = self.engine.picture().layers().cloned().collect();
        let entities: Vec<_> = changed
            .iter()
            .filter_map(|id| self.engine.picture().entity(id).cloned())
            .collect();
        self.durable.upsert_all(layers, entities)?;
        for observation in observations {
            self.remember(&observation.observation().id.0);
        }
        Ok(())
    }

    /// Fuse a verified observation and seal the entity it changed.
    pub fn fuse(&mut self, observation: &VerifiedObservation) -> Result<EntityId, StoreError> {
        let id = self.engine.ingest(observation);
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
        self.durable.upsert_entity(entity)?;
        // Remembered only once durable, so a submission that failed to persist can be
        // retried rather than refused as a replay.
        self.remember(&observation.observation().id.0);
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
    last_sweep: AtomicU64,
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
            last_sweep: AtomicU64::new(0),
        })
    }

    /// Remove entities not seen for `seconds` (0, the default, keeps them for ever).
    pub fn set_track_ttl(&self, seconds: u64) {
        self.track_ttl.store(seconds, Ordering::Relaxed);
    }

    /// Sweep stale tracks if a time-to-live is set and the last sweep is old enough;
    /// a sweep that removes anything is audited as `ExpireTracks`.
    fn sweep(&self, now: u64) {
        let ttl = self.track_ttl.load(Ordering::Relaxed);
        if ttl == 0
            || now.saturating_sub(self.last_sweep.load(Ordering::Relaxed)) < EXPIRY_SWEEP_SECS
        {
            return;
        }
        self.last_sweep.store(now, Ordering::Relaxed);
        let removed = match lock(&self.state).expire_before(now.saturating_sub(ttl)) {
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

    /// The router the PQ-mTLS gateway serves: [`INGEST`], [`INGEST_BATCH`] and
    /// [`READ_PICTURE`].
    pub fn router(self: &Arc<Self>) -> Router {
        let service = self.clone();
        let ingest: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.ingest(peer, body));
        let service = self.clone();
        let read: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, _: &[u8]| service.read_picture(peer));
        let service = self.clone();
        let batch: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.ingest_batch(peer, body));
        Router::new()
            .route(INGEST, ingest)
            .route(INGEST_BATCH, batch)
            .route(READ_PICTURE, read)
    }

    /// Admit, fuse and persist one signed observation.
    pub fn ingest(&self, peer: &WorkloadIdentity, body: &[u8]) -> Vec<u8> {
        let now = (self.clock)();
        let admitted = self.admit(peer, body, now, &BTreeSet::new());
        let (verified, audit_seq) = match admitted {
            Ok(ok) => ok,
            Err(Refusal { code, message }) => return self.reply(INGEST, code, &message),
        };
        let fused = lock(&self.state).fuse(&verified);
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
        let fused = lock(&self.state).fuse_many(&admitted);
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

    /// The picture as JSON: layers with visibility, and every tracked entity.
    pub fn read_picture(&self, peer: &WorkloadIdentity) -> Vec<u8> {
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
        let entities = picture.entities().map(entity_json).collect();
        let body = Json::obj(vec![
            ("layers", Json::Arr(layers)),
            ("entities", Json::Arr(entities)),
        ]);
        self.reply(READ_PICTURE, outcome::ACCEPTED, &body.encode())
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
