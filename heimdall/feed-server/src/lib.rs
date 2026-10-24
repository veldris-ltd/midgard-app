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
//! A refusal at 3 or 4 is audited too. Admitted observations are fused and the
//! picture is sealed at rest after each one, so a restart recovers it. [RC-13]
//!
//! Responses are one outcome byte ([`outcome`]) followed by UTF-8: a message, or for
//! [`READ_PICTURE`] the picture as JSON.

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

/// The method a source submits one encoded signed observation with.
pub const INGEST: &str = "Ingest";
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
}

impl PictureState {
    /// Resume fusion over a recovered picture: the engine starts from everything the
    /// durable picture holds, so later observations extend the recovered tracks.
    pub fn recover(durable: DurablePicture) -> Self {
        let mut engine = FusionEngine::new();
        PictureDelta::snapshot(durable.picture()).merge_into(engine.picture_mut());
        for layer in durable.picture().layers() {
            if !durable.picture().is_layer_visible(&layer.id) {
                engine.picture_mut().set_layer_visible(&layer.id, false);
            }
        }
        Self { engine, durable }
    }

    /// The current picture.
    pub fn picture(&self) -> &Picture {
        self.engine.picture()
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
        Arc::new(Self {
            state: Mutex::new(state),
            governance: Mutex::new(governance),
            provider,
            resource,
            purpose,
            clock,
            metrics,
        })
    }

    /// The router the PQ-mTLS gateway serves: [`INGEST`] and [`READ_PICTURE`].
    pub fn router(self: &Arc<Self>) -> Router {
        let service = self.clone();
        let ingest: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| service.ingest(peer, body));
        let service = self.clone();
        let read: Arc<dyn RequestHandler> =
            Arc::new(move |peer: &WorkloadIdentity, _: &[u8]| service.read_picture(peer));
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
        if feed != peer.id.0 {
            return self.refuse(
                peer,
                now,
                "Ingest refused: observation from another feed",
                &feed,
                "a source submits only its own feed's observations",
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
                )
            }
        };

        match lock(&self.state).fuse(&verified) {
            Ok(entity) => self.reply(
                INGEST,
                outcome::ACCEPTED,
                &format!(
                    "fused into entity {} at audit sequence {}",
                    entity.0, decision.audit_seq
                ),
            ),
            Err(e) => self.reply(
                INGEST,
                outcome::FAILED,
                &format!("the picture was not persisted: {e:?}"),
            ),
        }
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
    ) -> Vec<u8> {
        let recorded =
            lock(&self.governance).record_event(&peer.id.0, action, &format!("feed:{feed}"), now);
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
