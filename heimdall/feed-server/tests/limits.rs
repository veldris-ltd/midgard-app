//! The fed picture is bounded and every read of it fits the mesh frame cap: a flood
//! of distinct entity keys is held to the entity cap by evicting stale tracks and
//! then refused `picture-full` and audited; oversized attributes are malformed; a
//! picture read comes in pages the shipped analyst client can read; and hidden
//! layers are left out unless asked for. In process, over an in-memory ledger and a
//! scratch sealed picture. [FR-H-1] [FR-H-3] [RC-13] [VLD-ANB-MIDGARD-0017]
#![cfg(feature = "backend-fips")]

use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use attest::Keystore;
use gateway::Gateway;
use governance::Governance;
use heimdall_feed_gateway::{encode_signed_observation, sign_observation};
use heimdall_feed_server::{
    outcome, FeedService, IngestLimits, PageRequest, PictureState, READ_PICTURE,
};
use heimdall_picture::{
    Attr, EntityId, FeedId, LayerId, Observation, PictureLimits, Track, TrackedEntity,
};
use heimdall_store::DurablePicture;
use iam::{Idp, IDP_SIG_ALG};
use policy::{
    ClassificationLevel, DefaultPdp, Marking, PolicyDecisionPoint, Purpose, Resource, ResourceId,
    Role,
};
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity, MAX_RECORD_FRAME};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{OsRng, SignatureScheme, SigningKey};
use service_runtime::json::{parse, Json};
use service_runtime::policy_file::parse_policy;
use service_runtime::{client, ServiceIdentity};
use telemetry::MetricsRegistry;

const POLICY: &str = r#"{
  "version": 1,
  "rules": [
    { "id": "source-channel", "role": "feed-source", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "source-ingest", "role": "feed-source", "action": "Ingest",
      "resource": "*", "effect": "permit" },
    { "id": "analyst-channel", "role": "picture-reader", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "analyst-read", "role": "picture-reader", "action": "ReadPicture",
      "resource": "*", "effect": "permit" }
  ]
}"#;

const PURPOSE: &str = "situational-awareness";
/// The service clock: every observation below is received at this time.
const NOW: u64 = 1_000_000;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "heimdall-feed-limits-{tag}-{}-{uniq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pdp() -> DefaultPdp {
    DefaultPdp::new(parse_policy(POLICY).unwrap())
}

fn resource(id: &str) -> Resource {
    Resource {
        id: ResourceId(id.into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose(PURPOSE.into())].into_iter().collect(),
    }
}

/// A feed source: its mesh identity as the gateway hands it to the service, and the
/// key its certificate binds.
struct Source {
    peer: WorkloadIdentity,
    key: SigningKey,
}

fn source(name: &str) -> Source {
    let (verifying_key, key) = FipsSig::new(IDP_SIG_ALG).keypair(&mut OsRng).unwrap();
    Source {
        peer: WorkloadIdentity {
            id: ServiceId(name.into()),
            roles: [Role("feed-source".into())].into_iter().collect(),
            sig_alg: IDP_SIG_ALG,
            verifying_key,
        },
        key,
    }
}

fn analyst() -> WorkloadIdentity {
    let (verifying_key, _) = FipsSig::new(IDP_SIG_ALG).keypair(&mut OsRng).unwrap();
    WorkloadIdentity {
        id: ServiceId("analyst".into()),
        roles: [Role("picture-reader".into())].into_iter().collect(),
        sig_alg: IDP_SIG_ALG,
        verifying_key,
    }
}

/// An in-process feed service over a scratch sealed picture in `dir`.
fn service(
    dir: &Path,
    limits: PictureLimits,
) -> (
    Arc<FeedService<audit_writer::InMemoryLedger>>,
    Arc<MetricsRegistry>,
) {
    let durable = DurablePicture::open(dir.join("picture.sealed"), Box::new(TestSealer)).unwrap();
    let metrics = Arc::new(MetricsRegistry::new());
    let service = FeedService::new(
        PictureState::recover(durable).with_limits(limits),
        Governance::new(pdp(), &mut OsRng).unwrap(),
        Arc::new(RwLock::new(fips_provider())),
        resource("heimdall-feed"),
        Purpose(PURPOSE.into()),
        Arc::new(|| NOW),
        metrics.clone(),
        // The observations are dated in logical time, far behind NOW: the window
        // behind the clock is opened all the way (it is proved in ingest_window.rs).
        IngestLimits {
            max_age: u64::MAX,
            ..IngestLimits::default()
        },
    );
    (service, metrics)
}

/// A sealer that only frames: these tests are about bounds, not the cipher (the
/// store's own tests cover that).
struct TestSealer;

impl audit_writer::Sealer for TestSealer {
    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, audit_writer::AuditError> {
        Ok([aad, plaintext].concat())
    }
    fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, audit_writer::AuditError> {
        sealed
            .strip_prefix(aad)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| audit_writer::AuditError::Store("wrong aad".into()))
    }
}

fn submit(
    service: &FeedService<audit_writer::InMemoryLedger>,
    source: &Source,
    observation: Observation,
) -> (u8, String) {
    let signed = sign_observation(&FipsSig::new(IDP_SIG_ALG), &source.key, observation).unwrap();
    let response = service.ingest(&source.peer, &encode_signed_observation(&signed));
    (
        response[0],
        String::from_utf8_lossy(&response[1..]).into_owned(),
    )
}

fn observation(source: &Source, key: &str, kind: &str, at: u64) -> Observation {
    Observation::new(
        format!("{}-{key}-{at}", source.peer.id.0),
        FeedId(source.peer.id.0.clone()),
        kind,
        key,
        at,
    )
}

fn page(
    service: &FeedService<audit_writer::InMemoryLedger>,
    peer: &WorkloadIdentity,
    request: &[u8],
) -> (Json, usize) {
    let response = service.read_picture(peer, request);
    assert_eq!(
        response[0],
        outcome::ACCEPTED,
        "{}",
        String::from_utf8_lossy(&response[1..])
    );
    let body = std::str::from_utf8(&response[1..]).unwrap();
    (parse(body).expect("a page is JSON"), body.len())
}

fn total(page: &Json) -> i64 {
    page.get("total").and_then(Json::as_i64).unwrap()
}

fn ids(page: &Json) -> Vec<String> {
    match page.get("entities") {
        Some(Json::Arr(items)) => items
            .iter()
            .map(|e| e.str_field("id").unwrap().to_string())
            .collect(),
        _ => panic!("no entities in the page"),
    }
}

/// Distinct keys in the flood, ten times [`FLOOD_CAP`]. The book's figure is 10,000
/// against 1,000, but each observation here costs two ML-DSA signatures (the source's
/// and the audit entry's) and a verification, about 0.1 s in a debug build, which no
/// change to the picture can remove; the ratio and every assertion are the same.
const FLOOD_KEYS: u64 = 500;
/// The entity cap the flood runs against.
const FLOOD_CAP: usize = 50;

#[test]
fn a_flood_of_distinct_keys_ten_times_the_cap_keeps_the_picture_bounded() {
    // [VLD-ANB-MIDGARD-0017] [AC-44.1]
    let dir = scratch("flood");
    let stale = PictureLimits {
        max_entities: FLOOD_CAP,
        evict_after: 0,
        ..PictureLimits::default()
    };
    let (service, metrics) = service(&dir, stale);
    let radar = source("radar-1");

    // First half: every track is already stale, so each time the cap is reached the
    // stale tracks are evicted and the new key is fused.
    for i in 0..FLOOD_KEYS / 2 {
        let (code, message) = submit(
            &service,
            &radar,
            observation(&radar, &format!("T{i:05}"), "air-track", i),
        );
        assert_eq!(code, outcome::ACCEPTED, "{i}: {message}");
    }
    let evictions = metrics.counter_value("picture_evictions_total", &[]);
    assert!(evictions > 0, "stale tracks were evicted");

    // Second half: nothing is stale enough to evict, so a new key is refused.
    service.set_picture_limits(PictureLimits {
        evict_after: u64::MAX,
        ..stale
    });
    for i in FLOOD_KEYS / 2..FLOOD_KEYS {
        let (code, message) = submit(
            &service,
            &radar,
            observation(&radar, &format!("T{i:05}"), "air-track", i),
        );
        assert_eq!(code, outcome::DENIED, "{i}: {message}");
        assert!(message.contains("picture-full"), "{message}");
    }

    let reader = analyst();
    let (first, _) = page(&service, &reader, b"");
    assert_eq!(
        total(&first),
        FLOOD_CAP as i64,
        "the picture holds exactly the cap"
    );
    assert_eq!(
        metrics.counter_value("picture_evictions_total", &[]),
        evictions,
        "nothing was evicted once nothing was stale"
    );
    // A known key still updates its track when the picture is full.
    let held = ids(&first)[0].clone();
    let (code, message) = submit(
        &service,
        &radar,
        observation(&radar, &held, "air-track", 20_000),
    );
    assert_eq!(code, outcome::ACCEPTED, "{message}");

    let refusals = service
        .audited_actions()
        .iter()
        .filter(|a| a.as_str() == "Ingest refused: picture full")
        .count();
    assert_eq!(
        refusals as u64,
        FLOOD_KEYS / 2,
        "every picture-full refusal is audited"
    );
    assert!(
        metrics
            .render()
            .contains(&format!("picture_entities {FLOOD_CAP}\n")),
        "{}",
        metrics.render()
    );

    // Each change was a journal append: the whole picture was never resealed.
    assert!(dir.join("picture.journal").exists());
    assert!(
        !dir.join("picture.sealed").exists(),
        "no observation rewrote the whole picture"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn oversized_attributes_are_malformed() {
    // [VLD-ANB-MIDGARD-0017] [AC-44.4]
    let dir = scratch("attrs");
    let (service, _) = service(&dir, PictureLimits::default());
    let radar = source("radar-1");

    let mut many = observation(&radar, "MANY", "air-track", 100);
    for i in 0..65 {
        many = many.with_attr(format!("a{i:02}"), Attr::Int(i));
    }
    let (code, message) = submit(&service, &radar, many);
    assert_eq!(code, outcome::MALFORMED, "{message}");

    let big = observation(&radar, "BIG", "air-track", 100)
        .with_attr("note", Attr::Text("x".repeat(5_000)));
    let (code, message) = submit(&service, &radar, big);
    assert_eq!(code, outcome::MALFORMED, "{message}");

    let mut most = observation(&radar, "MOST", "air-track", 100)
        .with_attr("note", Attr::Text("x".repeat(4_000)));
    for i in 0..63 {
        most = most.with_attr(format!("a{i:02}"), Attr::Int(i));
    }
    let (code, message) = submit(&service, &radar, most);
    assert_eq!(code, outcome::ACCEPTED, "{message}");

    let (picture, _) = page(&service, &analyst(), b"");
    assert_eq!(ids(&picture), ["MOST"], "nothing oversized was fused");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hidden_layers_are_filtered_unless_asked() {
    // [VLD-ANB-MIDGARD-0017] [AC-44.4] [FR-H-1]
    let dir = scratch("hidden");
    let (service, _) = service(&dir, PictureLimits::default());
    let radar = source("radar-1");
    for (key, kind) in [("AIR-1", "air-track"), ("SEA-1", "sea-contact")] {
        let (code, message) = submit(&service, &radar, observation(&radar, key, kind, 100));
        assert_eq!(code, outcome::ACCEPTED, "{message}");
    }
    service
        .set_layer_visible(&LayerId("sea".into()), false)
        .unwrap();

    let reader = analyst();
    let (visible, _) = page(&service, &reader, b"");
    assert_eq!(ids(&visible), ["AIR-1"]);
    assert_eq!(total(&visible), 1);
    let (all, _) = page(&service, &reader, br#"{"includeHidden": true}"#);
    assert_eq!(ids(&all), ["AIR-1", "SEA-1"]);
    assert_eq!(total(&all), 2);
    // The layers are listed either way, with their visibility.
    let sea = match visible.get("layers") {
        Some(Json::Arr(layers)) => layers
            .iter()
            .find(|l| l.str_field("id") == Some("sea"))
            .cloned()
            .unwrap(),
        _ => panic!("no layers"),
    };
    assert_eq!(sea.get("visible"), Some(&Json::Bool(false)));

    let malformed = service.read_picture(&reader, br#"{"offset": -1}"#);
    assert_eq!(malformed[0], outcome::MALFORMED);
    let _ = std::fs::remove_dir_all(&dir);
}

// --- the shipped analyst client, over a real PQ-mTLS gateway ------------------------

/// What `mg-admin mesh issue` writes, for `service` under `idp`.
fn issue(idp: &Idp, dir: &Path, service: &str, roles: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    let key = Keystore::generate(service, IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(&format!("{service}-audit"), IDP_SIG_ALG, &mut OsRng).unwrap();
    let cert = idp
        .certify(
            &WorkloadIdentity {
                id: ServiceId(service.into()),
                roles: roles.iter().map(|r| Role(r.to_string())).collect(),
                sig_alg: IDP_SIG_ALG,
                verifying_key: key.verifying_key.clone(),
            },
            pq_mtls::CertTerms::new(
                1,
                0,
                u64::MAX,
                pq_mtls::Posture::at(ClassificationLevel::TOP_SECRET),
            ),
        )
        .unwrap();
    key.write(&dir.join(format!("{service}.keystore"))).unwrap();
    audit
        .write(&dir.join(format!("{service}.audit.keystore")))
        .unwrap();
    std::fs::write(dir.join(format!("{service}.cert")), encode_cert(&cert)).unwrap();
    std::fs::write(dir.join("mesh.anchor"), encode_anchor(&idp.trust_anchor())).unwrap();
    let crl = idp.sign_crl(1, 0, Default::default()).unwrap();
    std::fs::write(dir.join("mesh.crl"), pq_mtls::encode_crl(&crl)).unwrap();
}

/// An entity with 40 text attributes of 100 bytes each.
fn heavy_entity(i: usize) -> TrackedEntity {
    let latest_attrs: BTreeMap<String, Attr> = (0..40)
        .map(|a| {
            (
                format!("attr{a:02}"),
                Attr::Text(format!("{i:06}{a:02}").repeat(13)[..100].to_string()),
            )
        })
        .collect();
    TrackedEntity {
        id: EntityId(format!("E{i:05}")),
        entity_key: format!("E{i:05}"),
        kind: "air-track".into(),
        layer: LayerId("air".into()),
        sources: ["radar-1".to_string()].into_iter().collect(),
        track: Track::opened_at(i as u64, i as u64),
        classification: None,
        latest_attrs,
    }
}

#[test]
fn read_picture_pages_never_exceed_the_frame_cap() {
    // [VLD-ANB-MIDGARD-0017] [AC-44.3] [NFR-SEC-1]
    let dir = scratch("pages");
    {
        let mut durable =
            DurablePicture::open(dir.join("picture.sealed"), Box::new(TestSealer)).unwrap();
        for i in 0..5_000 {
            durable.upsert_entity(heavy_entity(i)).unwrap();
        }
    }
    let (service, _) = service(&dir, PictureLimits::default());

    // In process: every page body is under the frame cap and the pages cover every
    // entity exactly once.
    let reader = analyst();
    let mut seen = Vec::new();
    let mut request = PageRequest::default();
    loop {
        let (body, len) = page(&service, &reader, &request.encode());
        assert!(len < MAX_RECORD_FRAME, "a page of {len} bytes");
        assert_eq!(total(&body), 5_000);
        let returned = ids(&body);
        assert!(!returned.is_empty());
        let truncated = body.get("truncated") == Some(&Json::Bool(true));
        seen.extend(returned.iter().cloned());
        request.offset += returned.len();
        assert_eq!(truncated, request.offset < 5_000);
        if !truncated {
            break;
        }
    }
    let expected: Vec<String> = (0..5_000).map(|i| format!("E{i:05}")).collect();
    assert_eq!(seen, expected, "every entity exactly once, in id order");

    // Over the mesh: the shipped analyst client reads every page, at the largest
    // page size it may ask for.
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let identities = dir.join("identities");
    issue(&idp, &identities, "heimdall-feed", &["feed-gateway"]);
    issue(&idp, &identities, "analyst", &["picture-reader"]);
    let server = ServiceIdentity::load(&identities, "heimdall-feed").unwrap();
    let channel_pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(pdp());
    let gateway = Gateway::new(
        Arc::new(fips_provider()),
        server.cert.clone(),
        server.signing_key.clone(),
        server.anchor.clone(),
        channel_pdp,
        resource("heimdall-feed-channel"),
        Arc::new(service.router()),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        service_runtime::serve_mesh(
            "heimdall-feed",
            listener,
            gateway,
            Arc::new(MetricsRegistry::new()),
        )
    });

    let analyst = ServiceIdentity::load(&identities, "analyst").unwrap();
    let mut request = PageRequest {
        limit: heimdall_feed_server::MAX_PAGE_LIMIT,
        ..PageRequest::default()
    };
    let mut read = 0;
    let mut pages = 0;
    loop {
        let response = client::call(
            &analyst,
            &addr,
            "heimdall-feed",
            PURPOSE,
            READ_PICTURE,
            &request.encode(),
        )
        .expect("the analyst client reads the page");
        assert_eq!(response[0], outcome::ACCEPTED);
        let body = parse(std::str::from_utf8(&response[1..]).unwrap()).unwrap();
        let returned = ids(&body).len();
        read += returned;
        pages += 1;
        request.offset += returned;
        if body.get("truncated") != Some(&Json::Bool(true)) {
            break;
        }
    }
    assert_eq!(read, 5_000);
    assert!(pages > 1, "the picture does not fit one page");
    let _ = std::fs::remove_dir_all(&dir);
}
