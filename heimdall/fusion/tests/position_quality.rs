//! Position quality in fusion: an entity's ADS-B position outranks a derived one (MLAT,
//! TIS-B, Mode S) for a minute of observation time; the derived observation still
//! merges its other attributes, counts, and records its feed; and the entity says which
//! feed and source supplied its current position. [FR-H-1] [FR-H-2]
#![cfg(feature = "backend-fips")]

use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::{shared, CryptoProvider};
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey};

use heimdall_feed_gateway::{sign_observation, FeedGateway, FeedRegistry, VerifiedObservation};
use heimdall_fusion::{FusionEngine, POS_AT, POS_FEED, POS_SOURCE};
use heimdall_picture::{Attr, EntityId, FeedId, Observation, TrackedEntity};

const ALG: SigId = SigId::MlDsa65;

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            *b = (self.0 >> 33) as u8;
        }
    }
}

struct Feeds {
    gateway: FeedGateway,
    keys: Vec<(String, SigningKey)>,
}

impl Feeds {
    fn new(names: &[&str]) -> Self {
        let mut registry = FeedRegistry::new();
        let mut keys = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let (vk, sk) = FipsSig::new(ALG)
                .keypair(&mut SeedRng(i as u64 + 7))
                .unwrap();
            registry.register(FeedId(name.to_string()), vk);
            keys.push((name.to_string(), sk));
        }
        let mut p = CryptoProvider::new();
        p.register_sig(Box::new(FipsSig::new(ALG)));
        Self {
            gateway: FeedGateway::new(registry, shared(p)),
            keys,
        }
    }

    /// A verified observation of `AC-1` from `feed` at `t`.
    fn at(&self, feed: &str, t: u64, attrs: &[(&str, Attr)]) -> VerifiedObservation {
        let mut o = Observation::new(
            format!("{feed}-{t}"),
            FeedId(feed.into()),
            "air-track",
            "AC-1",
            t,
        );
        for (k, v) in attrs {
            o = o.with_attr(*k, v.clone());
        }
        let sk = &self.keys.iter().find(|(n, _)| n == feed).unwrap().1;
        self.gateway
            .authenticate(&sign_observation(&FipsSig::new(ALG), sk, o).unwrap())
            .unwrap()
    }
}

fn pos(lat: f64, lon: f64, source: Option<&str>) -> Vec<(&'static str, Attr)> {
    let mut v = vec![
        ("lat", Attr::Float(lat)),
        ("lon", Attr::Float(lon)),
        ("speed", Attr::Float(lat * 10.0)),
        ("track", Attr::Float(90.0)),
    ];
    if let Some(s) = source {
        v.push((POS_SOURCE, Attr::Text(s.into())));
    }
    v
}

fn ac1(engine: &FusionEngine) -> &TrackedEntity {
    engine.picture().entity(&EntityId("AC-1".into())).unwrap()
}

fn attr<'a>(e: &'a TrackedEntity, k: &str) -> Option<&'a Attr> {
    e.latest_attrs.get(k)
}

#[test]
fn an_adsb_position_outranks_a_derived_one_for_a_minute() {
    let feeds = Feeds::new(&["adsb-1", "mlat-1", "radar"]);
    let mut engine = FusionEngine::new();

    let first = engine.fuse(&feeds.at("adsb-1", 100, &pos(51.0, -1.0, Some("adsb_icao"))));
    assert!(first.position_adopted);
    let e = ac1(&engine);
    assert_eq!(attr(e, POS_FEED), Some(&Attr::Text("adsb-1".into())));
    assert_eq!(attr(e, POS_AT), Some(&Attr::Int(100)));

    // MLAT 30 s later: the ADS-B position, speed and track stay; the squawk merges.
    let mut mlat = pos(52.0, -2.0, Some("mlat"));
    mlat.push(("squawk", Attr::Text("7700".into())));
    let held = engine.fuse(&feeds.at("mlat-1", 130, &mlat));
    assert!(
        !held.position_adopted,
        "a held position is not a track point"
    );
    let e = ac1(&engine);
    assert_eq!(attr(e, "lat"), Some(&Attr::Float(51.0)));
    assert_eq!(attr(e, "lon"), Some(&Attr::Float(-1.0)));
    assert_eq!(attr(e, "speed"), Some(&Attr::Float(510.0)));
    assert_eq!(attr(e, POS_SOURCE), Some(&Attr::Text("adsb_icao".into())));
    assert_eq!(attr(e, POS_FEED), Some(&Attr::Text("adsb-1".into())));
    assert_eq!(attr(e, POS_AT), Some(&Attr::Int(100)));
    assert_eq!(attr(e, "squawk"), Some(&Attr::Text("7700".into())));
    assert_eq!(e.track.observation_count, 2);
    assert_eq!(e.track.last_seen, 130);
    assert!(e.sources.contains("mlat-1"), "the source is still recorded");

    // A TIS-B position from the same ADS-B feed is held too: it is the source, not the
    // feed, that ranks.
    assert!(
        !engine
            .fuse(&feeds.at("adsb-1", 140, &pos(53.0, -3.0, Some("tisb_icao"))))
            .position_adopted
    );
    // Another ADS-B position (even a non-transponder `adsb_icao_nt`) is adopted.
    assert!(
        engine
            .fuse(&feeds.at("mlat-1", 150, &pos(51.1, -1.1, Some("adsb_icao_nt"))))
            .position_adopted
    );
    let e = ac1(&engine);
    assert_eq!(attr(e, "lat"), Some(&Attr::Float(51.1)));
    assert_eq!(attr(e, POS_FEED), Some(&Attr::Text("mlat-1".into())));
    assert_eq!(attr(e, POS_AT), Some(&Attr::Int(150)));

    // More than a minute after the last ADS-B position, MLAT is adopted.
    assert!(
        engine
            .fuse(&feeds.at("mlat-1", 211, &pos(54.0, -4.0, Some("mlat"))))
            .position_adopted
    );
    let e = ac1(&engine);
    assert_eq!(attr(e, "lat"), Some(&Attr::Float(54.0)));
    assert_eq!(attr(e, POS_SOURCE), Some(&Attr::Text("mlat".into())));
    // And now that the position is MLAT, more MLAT is adopted at once.
    assert!(
        engine
            .fuse(&feeds.at("mlat-1", 212, &pos(54.1, -4.0, Some("mode_s"))))
            .position_adopted
    );
}

#[test]
fn a_position_without_a_source_is_adopted_as_before() {
    let feeds = Feeds::new(&["adsb-1", "radar"]);
    let mut engine = FusionEngine::new();
    engine.fuse(&feeds.at("adsb-1", 100, &pos(51.0, -1.0, Some("adsb_icao"))));
    // No pos_source: adopted even over a fresh ADS-B position, and it has no source.
    let plain = engine.fuse(&feeds.at("radar", 101, &pos(50.0, 0.5, None)));
    assert!(plain.position_adopted);
    let e = ac1(&engine);
    assert_eq!(attr(e, "lat"), Some(&Attr::Float(50.0)));
    assert_eq!(attr(e, POS_SOURCE), None);
    assert_eq!(attr(e, POS_FEED), Some(&Attr::Text("radar".into())));

    // An observation with no position carries no position stamp, and cannot forge one.
    let unpositioned = engine.fuse(&feeds.at(
        "radar",
        102,
        &[
            ("callsign", Attr::Text("ALPHA1".into())),
            (POS_FEED, Attr::Text("someone-else".into())),
        ],
    ));
    assert!(!unpositioned.position_adopted);
    let e = ac1(&engine);
    assert_eq!(attr(e, POS_FEED), None);
    assert_eq!(attr(e, "callsign"), Some(&Attr::Text("ALPHA1".into())));

    // A derived position arriving out of order near an ADS-B one is not a track point.
    engine.fuse(&feeds.at("adsb-1", 200, &pos(51.0, -1.0, Some("adsb_icao"))));
    let late = engine.fuse(&feeds.at("radar", 190, &pos(55.0, 1.0, Some("mlat"))));
    assert!(!late.position_adopted);
    assert_eq!(attr(ac1(&engine), "lat"), Some(&Attr::Float(51.0)));
}
