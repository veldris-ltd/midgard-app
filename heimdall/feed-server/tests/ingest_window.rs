//! The feed service bounds what a certified feed's clock can do to the picture, in
//! process against a fixed clock: an observation dated far ahead of or far behind the
//! server's receipt time is refused and audited with its reason and entity key, a
//! near-future observation does not freeze its track, a replayed observation id is
//! refused, and the picture reports when each entity was last received and whether it
//! has gone stale. [VLD-ANB-MIDGARD-0015] [FR-H-1] [FR-H-3] [NFR-SEC-1]
//!
//! radar-1 numbers its observations with a feed sequencer; resubmitting the same
//! signed bytes meets the sequence check, an observation with no sequence is refused,
//! and each feed's last sequence is saved so a restarted service still refuses a
//! replay. [VLD-ANB-MIDGARD-0041] [AC-59.5]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use audit_writer::{AuditError, Outcome, Sealer};
use governance::Governance;
use heimdall_feed_gateway::{encode_signed_observation, sign_observation, FeedSequencer};
use heimdall_feed_server::{
    outcome, FeedService, IngestLimits, PictureState, SequenceFile, REASON_OUT_OF_WINDOW,
    REASON_REPLAY, REASON_SEQUENCE_UNSET,
};
use heimdall_picture::{Attr, FeedId, Observation};
use heimdall_store::DurablePicture;
use iam::IDP_SIG_ALG;
use policy::{ClassificationLevel, DefaultPdp, Marking, Purpose, Resource, ResourceId, Role};
use pq_mtls::{Authorization, Posture, ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{OsRng, SignatureScheme, SigningKey};
use service_runtime::json::{parse, Json};
use service_runtime::policy_file::parse_policy;
use telemetry::MetricsRegistry;

/// The rules `tests/serve.rs` runs the service under.
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
/// The server's clock, in seconds.
const NOW: u64 = 1_800_000_000;
const ENTITY: &str = "AC-1";

/// Binds the AAD without a KEM backend: these tests are about admission, not sealing.
struct AadSealer;
impl Sealer for AadSealer {
    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
        let mut o = (aad.len() as u32).to_le_bytes().to_vec();
        o.extend_from_slice(aad);
        o.extend_from_slice(plaintext);
        Ok(o)
    }
    fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
        let n = sealed
            .get(..4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| AuditError::Store("short".into()))?;
        if sealed.get(4..4 + n) != Some(aad) {
            return Err(AuditError::Store("aad mismatch".into()));
        }
        Ok(sealed[4 + n..].to_vec())
    }
}

/// A feed service over an in-memory ledger and a picture in a scratch directory, with
/// a clock the test moves, one certified feed source and one analyst.
struct Fixture {
    service: Arc<FeedService<audit_writer::InMemoryLedger>>,
    clock: Arc<AtomicU64>,
    radar: WorkloadIdentity,
    radar_key: SigningKey,
    radar_sequence: FeedSequencer,
    analyst: WorkloadIdentity,
    dir: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn identity(service: &str, role: &str) -> (WorkloadIdentity, SigningKey) {
    let (verifying_key, signing_key) = FipsSig::new(IDP_SIG_ALG).keypair(&mut OsRng).unwrap();
    (
        WorkloadIdentity {
            id: ServiceId(service.into()),
            roles: [Role(role.into())].into_iter().collect(),
            sig_alg: IDP_SIG_ALG,
            verifying_key,
        },
        signing_key,
    )
}

fn fixture(tag: &str) -> Fixture {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "heimdall-feed-limits-{tag}-{}-{uniq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let (radar, radar_key) = identity("radar-1", "feed-source");
    fixture_in(dir, radar, radar_key)
}

/// A service over the picture and sequence file in `dir`, as if (re)started there.
fn fixture_in(dir: PathBuf, radar: WorkloadIdentity, radar_key: SigningKey) -> Fixture {
    let durable = DurablePicture::open(dir.join("picture.sealed"), Box::new(AadSealer)).unwrap();
    let (file, sequences) =
        SequenceFile::open(dir.join("feed-sequences.sealed"), Box::new(AadSealer)).unwrap();
    let governance =
        Governance::new(DefaultPdp::new(parse_policy(POLICY).unwrap()), &mut OsRng).unwrap();
    let clock = Arc::new(AtomicU64::new(NOW));
    let reading = clock.clone();
    let service = FeedService::new(
        PictureState::recover(durable).with_sequences(file, sequences),
        governance,
        Arc::new(RwLock::new(fips_provider())),
        Resource {
            id: ResourceId("heimdall-feed".into()),
            marking: Marking {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            allowed_purposes: [Purpose(PURPOSE.into())].into_iter().collect(),
        },
        Purpose(PURPOSE.into()),
        Arc::new(move || reading.load(Ordering::SeqCst)),
        Arc::new(MetricsRegistry::new()),
        IngestLimits::default(),
    );
    let (analyst, _) = identity("analyst", "picture-reader");
    Fixture {
        service,
        clock,
        radar,
        radar_key,
        radar_sequence: FeedSequencer::new(),
        analyst,
        dir,
    }
}

impl Fixture {
    /// The encoded signed observation radar-1 submits for the entity at `observed_at`,
    /// with radar-1's next sequence.
    fn signed(&self, id: &str, observed_at: u64, speed: i64) -> Vec<u8> {
        self.signed_with(id, observed_at, speed, self.radar_sequence.next())
    }

    /// As [`Fixture::signed`], with the sequence given.
    fn signed_with(&self, id: &str, observed_at: u64, speed: i64, sequence: u64) -> Vec<u8> {
        let observation = Observation::new(
            id,
            FeedId("radar-1".into()),
            "air-track",
            ENTITY,
            observed_at,
        )
        .with_attr("speed", Attr::Int(speed))
        .with_sequence(sequence);
        encode_signed_observation(
            &sign_observation(&FipsSig::new(IDP_SIG_ALG), &self.radar_key, observation).unwrap(),
        )
    }

    /// Submit as radar-1: the outcome byte and the message.
    fn ingest(&self, body: &[u8]) -> (u8, String) {
        let response = self.service.ingest(&self.radar, &channel(), body);
        (
            response[0],
            String::from_utf8_lossy(&response[1..]).into_owned(),
        )
    }

    /// The picture as the analyst reads it.
    fn picture(&self) -> Json {
        let response = self.service.read_picture(&self.analyst, &channel(), b"");
        assert_eq!(response[0], outcome::ACCEPTED);
        parse(&String::from_utf8_lossy(&response[1..])).expect("the picture is JSON")
    }

    /// The entity as the picture shows it, if it is there.
    fn entity(&self) -> Option<Json> {
        match self.picture().get("entities") {
            Some(Json::Arr(items)) => items
                .iter()
                .find(|e| e.str_field("id") == Some(ENTITY))
                .cloned(),
            _ => None,
        }
    }

    /// The last ledger row: its action, outcome, reason and target.
    fn last_row(&self) -> (String, Outcome, Option<String>, Option<String>) {
        let entries = self.service.audit_entries();
        let record = &entries.last().expect("a ledger row").record;
        (
            record.action.0.clone(),
            record.context.outcome,
            record.context.reason.clone(),
            record.context.target.clone(),
        )
    }
}

/// An observation dated ten years ahead of the server's clock is refused, never fused,
/// and its refusal is in the ledger with its reason and the entity key. [AC-23.1]

/// What the channel authorised a peer with a certificate issued before attributes for:
/// unclassified, binding no purposes. [VLD-ANB-MIDGARD-0063]
fn channel() -> Authorization {
    Authorization {
        clearance: Posture::unclassified(),
        purpose: Purpose("situational-awareness".into()),
        purposes: None,
    }
}

#[test]
fn an_observation_ten_years_ahead_is_refused_and_audited() {
    let f = fixture("future");
    let (code, message) = f.ingest(&f.signed("far-future", NOW + 315_360_000, 999));
    assert_eq!(code, outcome::DENIED, "{message}");
    assert!(message.contains("outside the accepted window"), "{message}");
    assert!(f.entity().is_none(), "nothing was fused");
    // The last row before the read is the refusal; the read adds its own decision.
    let entries = f.service.audit_entries();
    let refusal = &entries[entries.len() - 2].record;
    assert_eq!(
        refusal.action.0,
        "Ingest refused: observed_at outside the accepted window"
    );
    assert_eq!(refusal.context.outcome, Outcome::Refused);
    assert_eq!(
        refusal.context.reason.as_deref(),
        Some(REASON_OUT_OF_WINDOW)
    );
    assert_eq!(refusal.context.target.as_deref(), Some(ENTITY));
    assert_eq!(refusal.resource.0, "feed:radar-1");
}

/// One dated eight days behind the server's clock, older than the seven-day window, is
/// refused the same way. [AC-23.1]
#[test]
fn an_observation_older_than_the_retention_window_is_refused() {
    let f = fixture("old");
    let (code, message) = f.ingest(&f.signed("eight-days-old", NOW - 8 * 86_400, 300));
    assert_eq!(code, outcome::DENIED, "{message}");
    assert_eq!(
        f.last_row(),
        (
            "Ingest refused: observed_at outside the accepted window".to_string(),
            Outcome::Refused,
            Some(REASON_OUT_OF_WINDOW.to_string()),
            Some(ENTITY.to_string()),
        )
    );
    assert!(f.entity().is_none(), "nothing was fused");
    // Six days behind is inside the window.
    let (code, message) = f.ingest(&f.signed("six-days-old", NOW - 6 * 86_400, 300));
    assert_eq!(code, outcome::ACCEPTED, "{message}");
}

/// An observation 200 s ahead, inside the skew allowance, is accepted, and a later
/// genuine one still becomes the entity's latest state: the most a feed can run
/// ahead is the allowance, not years. [AC-23.2]
#[test]
fn a_near_future_observation_does_not_freeze_the_track() {
    let f = fixture("near-future");
    let (code, message) = f.ingest(&f.signed("ahead", NOW + 200, 420));
    assert_eq!(code, outcome::ACCEPTED, "{message}");
    let (code, message) = f.ingest(&f.signed("later", NOW + 250, 430));
    assert_eq!(code, outcome::ACCEPTED, "{message}");
    let entity = f.entity().expect("the entity is tracked");
    assert_eq!(
        entity.get("attributes").and_then(|a| a.get("speed")),
        Some(&Json::Int(430))
    );
    assert_eq!(entity.get("lastSeen"), Some(&Json::Int((NOW + 250) as i64)));
    assert_eq!(entity.get("observations"), Some(&Json::Int(2)));
}

/// The same signed bytes submitted twice: the second is refused as a replay (its
/// sequence is not above the last), audited with reason `replay`, and the track counts
/// the observation once. An id repeated under a new sequence is refused the same way.
/// [AC-23.3] [AC-59.1]
#[test]
fn a_replayed_observation_id_is_refused_once_per_feed_ring() {
    let f = fixture("replay");
    let body = f.signed("obs-1", NOW, 420);
    let (code, message) = f.ingest(&body);
    assert_eq!(code, outcome::ACCEPTED, "{message}");
    let (code, message) = f.ingest(&body);
    assert_eq!(code, outcome::DENIED, "{message}");
    assert!(message.contains("already been received"), "{message}");
    assert_eq!(
        f.last_row(),
        (
            "Ingest refused: replayed observation".to_string(),
            Outcome::Refused,
            Some(REASON_REPLAY.to_string()),
            Some(ENTITY.to_string()),
        )
    );
    assert!(message.contains("sequence 1"), "{message}");
    let (code, message) = f.ingest(&f.signed("obs-1", NOW, 420));
    assert_eq!(code, outcome::DENIED, "{message}");
    assert_eq!(
        message,
        "this observation has already been received from this feed"
    );
    let entity = f.entity().expect("the entity is tracked");
    assert_eq!(entity.get("observations"), Some(&Json::Int(1)));
    // A different observation from the same feed is still accepted.
    let (code, message) = f.ingest(&f.signed("obs-2", NOW + 1, 430));
    assert_eq!(code, outcome::ACCEPTED, "{message}");
}

/// An entity last received at NOW, read 1000 s later (past the 900 s default), is
/// shown as stale, with the receipt time; read straight away, it is not. [AC-23.4]
#[test]
fn read_picture_exposes_staleness() {
    let f = fixture("stale");
    // Dated a minute before receipt: `lastReceived` is the server's time, not the feed's.
    let (code, message) = f.ingest(&f.signed("obs-1", NOW - 60, 420));
    assert_eq!(code, outcome::ACCEPTED, "{message}");
    let fresh = f.entity().expect("the entity is tracked");
    assert_eq!(fresh.get("stale"), Some(&Json::Bool(false)));
    assert_eq!(fresh.get("lastReceived"), Some(&Json::Int(NOW as i64)));
    assert_eq!(fresh.get("lastSeen"), Some(&Json::Int((NOW - 60) as i64)));

    f.clock.store(NOW + 1000, Ordering::SeqCst);
    let stale = f.entity().expect("the entity is tracked");
    assert_eq!(stale.get("stale"), Some(&Json::Bool(true)));
    assert_eq!(stale.get("lastReceived"), Some(&Json::Int(NOW as i64)));
}

/// An observation built without a sequence (0) is refused and audited with reason
/// `sequence-unset`. [AC-59.1]
#[test]
fn an_observation_without_a_sequence_is_refused() {
    let f = fixture("unset");
    let (code, message) = f.ingest(&f.signed_with("obs-1", NOW, 420, 0));
    assert_eq!(code, outcome::DENIED, "{message}");
    assert_eq!(
        f.last_row(),
        (
            "Ingest refused: observation has no sequence".to_string(),
            Outcome::Refused,
            Some(REASON_SEQUENCE_UNSET.to_string()),
            Some(ENTITY.to_string()),
        )
    );
    assert!(f.entity().is_none(), "nothing was fused");
}

/// Each feed's last sequence is sealed before the observation is fused: a service
/// restarted over the same state directory still refuses the observation it admitted
/// before, while the feed's next sequence is accepted. [AC-59.5]
#[test]
fn replay_protection_survives_a_restart() {
    let f = fixture("restart");
    let body = f.signed_with("obs-1", NOW, 420, 7);
    let (code, message) = f.ingest(&body);
    assert_eq!(code, outcome::ACCEPTED, "{message}");

    let next = f.signed_with("obs-2", NOW, 430, 8);

    // Restart: a new service over the same directory, for the same radar-1 identity.
    let (_, unused_key) = identity("unused", "feed-source");
    let restarted = fixture_in(f.dir.clone(), f.radar.clone(), unused_key);
    let (code, message) = restarted.ingest(&body);
    assert_eq!(code, outcome::DENIED, "{message}");
    assert!(message.contains("sequence 7"), "{message}");
    let (code, message) = restarted.ingest(&next);
    assert_eq!(code, outcome::ACCEPTED, "{message}");
}
