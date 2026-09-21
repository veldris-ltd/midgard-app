//! Heimdall fusion acceptance — the offline core of Phase 6.b.
//!
//! - AC-H-1: multiple feeds fuse into one picture with per-layer toggles. [FR-H-1]
//! - AC-H-6: Mimir absent — fusion + rules-based classification stay functional. [DD-6]
//! - AC-H-7: no outbound connectivity — the whole verify→fuse→classify→project path
//!   runs locally. Run the suite under `docker --network none` to demonstrate it
//!   concretely. [NFR-PRV-1]
//!
//! The whole path uses the real FIPS backend for feed signatures, so this is not a
//! mock: unauthenticated observations could not have reached fusion.
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;

use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::{shared, CryptoProvider};
use pqc_agility::{Rng, SharedProvider, SigId, SignatureScheme, SigningKey, VerifyingKey};

use fabric::store::{InMemoryStore, ObjectStore};
use policy::{ClassificationLevel, Marking, Purpose};

use heimdall_classify::RulesClassifier;
use heimdall_feed_gateway::{sign_observation, FeedGateway, FeedRegistry, VerifiedObservation};
use heimdall_fusion::{sync_to_fabric, FusionEngine, ENTITY_TYPE_ID};
use heimdall_picture::{Attr, EntityId, FeedId, LayerId, Observation};

const ALG: SigId = SigId::MlDsa65;

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let b = z.to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&b[..take]);
            i += take;
        }
    }
}

fn provider() -> SharedProvider {
    let mut p = CryptoProvider::new();
    p.register_sig(Box::new(FipsSig::new(ALG)));
    shared(p)
}

fn keypair(seed: u64) -> (VerifyingKey, SigningKey) {
    FipsSig::new(ALG).keypair(&mut SeedRng(seed)).unwrap()
}

/// A little harness: a gateway trusting two feeds, plus a signer per feed.
struct Harness {
    gateway: FeedGateway,
    radar_sk: SigningKey,
    sonar_sk: SigningKey,
}

impl Harness {
    fn new() -> Self {
        let (radar_vk, radar_sk) = keypair(1);
        let (sonar_vk, sonar_sk) = keypair(2);
        let mut registry = FeedRegistry::new();
        registry.register(FeedId("radar".into()), radar_vk);
        registry.register(FeedId("sonar".into()), sonar_vk);
        Self {
            gateway: FeedGateway::new(registry, provider()),
            radar_sk,
            sonar_sk,
        }
    }

    fn verified(&self, feed: &str, obs: Observation) -> VerifiedObservation {
        let sk = if feed == "radar" {
            &self.radar_sk
        } else {
            &self.sonar_sk
        };
        let signed = sign_observation(&FipsSig::new(ALG), sk, obs).unwrap();
        self.gateway
            .authenticate(&signed)
            .expect("feed authenticates")
    }
}

fn obs(feed: &str, kind: &str, key: &str, t: u64, speed: f64) -> Observation {
    Observation::new(
        format!("{feed}-{key}-{t}"),
        FeedId(feed.into()),
        kind,
        key,
        t,
    )
    .with_attr("speed", Attr::Float(speed))
}

#[test]
fn ac_h_1_multiple_feeds_fuse_into_one_layered_picture_with_toggles() {
    let h = Harness::new();
    let mut engine =
        FusionEngine::new().with_classifier(Box::new(RulesClassifier::with_default_rules()));

    // Two feeds, two domains. Entity "A123" is observed twice (once per sweep) and
    // must fuse into a single tracked entity, not two.
    engine.ingest(&h.verified("radar", obs("radar", "air-track", "A123", 100, 480.0)));
    engine.ingest(&h.verified("radar", obs("radar", "air-track", "A124", 101, 100.0)));
    engine.ingest(&h.verified("sonar", obs("sonar", "sea-track", "S9", 102, 12.0)));
    engine.ingest(&h.verified("radar", obs("radar", "air-track", "A123", 105, 490.0)));

    let picture = engine.picture();

    // Three distinct entities across the two feeds — A123's two observations fused.
    assert_eq!(picture.entity_count(), 3);
    let a123 = picture
        .entity(&EntityId("A123".into()))
        .expect("A123 tracked");
    assert_eq!(a123.track.observation_count, 2);
    assert_eq!(a123.track.first_seen, 100);
    assert_eq!(a123.track.last_seen, 105);

    // Layers were derived from kinds: air + sea.
    assert!(picture.is_layer_visible(&LayerId("air".into())));
    assert_eq!(picture.layer_count(), 2);

    // All three visible by default.
    assert_eq!(picture.visible_count(), 3);

    // Per-layer toggle: hide the maritime layer → only the two air tracks remain.
    let mut engine = engine;
    engine
        .picture_mut()
        .set_layer_visible(&LayerId("sea".into()), false);
    assert_eq!(engine.picture().visible_count(), 2);
    assert!(engine
        .picture()
        .visible_entities()
        .all(|e| e.layer == LayerId("air".into())));

    // Toggling it back restores the full picture.
    engine
        .picture_mut()
        .set_layer_visible(&LayerId("sea".into()), true);
    assert_eq!(engine.picture().visible_count(), 3);
}

#[test]
fn ac_h_6_rules_classification_without_mimir() {
    // No Mimir anywhere — only the deterministic rules classifier.
    let h = Harness::new();
    let mut engine =
        FusionEngine::new().with_classifier(Box::new(RulesClassifier::with_default_rules()));

    engine.ingest(&h.verified("radar", obs("radar", "air-track", "A123", 100, 480.0)));
    engine.ingest(&h.verified("radar", obs("radar", "air-track", "A124", 101, 100.0)));
    engine.ingest(&h.verified("sonar", obs("sonar", "sea-track", "S9", 102, 12.0)));

    let picture = engine.picture();

    // Fast air track → classified "fast-mover" by rule, tagged as rules-sourced.
    let a123 = picture.entity(&EntityId("A123".into())).unwrap();
    let c = a123.classification.as_ref().expect("classified");
    assert_eq!(c.label, "fast-mover");
    assert_eq!(c.source, heimdall_picture::ClassificationSource::Rules);

    // Sea track → "surface-contact".
    let s9 = picture.entity(&EntityId("S9".into())).unwrap();
    assert_eq!(s9.classification.as_ref().unwrap().label, "surface-contact");

    // Slow, unremarkable air track → no rule fires → unclassified (still tracked).
    let a124 = picture.entity(&EntityId("A124".into())).unwrap();
    assert!(a124.classification.is_none());
}

#[test]
fn ac_h_7_full_path_runs_offline_and_projects_to_fabric() {
    // The whole path — sign, verify, fuse, classify, project into the object fabric
    // — executes with no network. The test performs zero outbound I/O by
    // construction; under `docker --network none` this proves the air-gap claim.
    let h = Harness::new();
    let mut engine =
        FusionEngine::new().with_classifier(Box::new(RulesClassifier::with_default_rules()));
    engine.ingest(&h.verified("radar", obs("radar", "air-track", "A123", 100, 480.0)));
    engine.ingest(&h.verified("sonar", obs("sonar", "sea-track", "S9", 102, 12.0)));

    // Project the fused, classified picture into the fabric with a marking.
    let marking = Marking {
        level: ClassificationLevel::SECRET,
        compartments: BTreeSet::new(),
    };
    let mut purposes = BTreeSet::new();
    purposes.insert(Purpose("operations".into()));

    let mut store = InMemoryStore::new();
    let written = sync_to_fabric(engine.picture(), &mut store, &marking, &purposes);
    assert_eq!(written, 2);

    // The projected objects carry the marking and the classification.
    let objs = store.objects_of_type(ENTITY_TYPE_ID);
    assert_eq!(objs.len(), 2);
    let a123 = store.get_object("A123").expect("A123 projected");
    assert_eq!(a123.marking, marking);
    assert!(a123
        .allowed_purposes
        .contains(&Purpose("operations".into())));
    assert_eq!(
        a123.properties.get("classification"),
        Some(&fabric::value::Value::Text("fast-mover".into()))
    );
}
