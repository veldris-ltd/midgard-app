//! RC-13: the fused picture and in-flight workflows survive a Heimdall restart —
//! recovered intact, with at-rest bytes as ciphertext, failing closed on tamper.
//! [FR-H-1] [FR-H-5] [RC-13]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use audit_writer::{verify_chain, LedgerId};
use heimdall_picture::{
    Classification, ClassificationSource, EntityId, Layer, LayerId, Track, TrackedEntity,
};
use heimdall_store::{DurableHistory, DurablePicture, DurableWorkflow, HistoryRecord};
use heimdall_workflow::{DispatchActuator, Effector, ProposedAction, WorkflowState};

/// A success effector for the durable-restart workflow's action type.
struct OkEffector;
impl Effector for OkEffector {
    fn effect(&mut self, action: &ProposedAction) -> Result<String, String> {
        Ok(format!("actuated {}", action.action_type))
    }
}
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{CryptoProvider, Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

const ROOT_SEED: u64 = 31337;
const SIG_ALG: SigId = SigId::MlDsa65;

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

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "heimdall-store-{tag}-{}-{}",
        std::process::id(),
        uniq
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root =
        SoftwareTrustRoot::generate(&provider, "heimdall-root", &mut SeedRng(ROOT_SEED)).unwrap();
    KmsSealer::new(Kms::new(provider, root))
}

fn entity(id: &str, layer: &str, feed: &str, label: &str, last_seen: u64) -> TrackedEntity {
    let mut sources = BTreeSet::new();
    sources.insert(feed.to_string());
    TrackedEntity {
        id: EntityId(id.into()),
        entity_key: id.into(),
        kind: "vessel".into(),
        layer: LayerId(layer.into()),
        sources,
        track: Track {
            first_seen: 100,
            last_seen,
            observation_count: 2,
        },
        classification: Some(Classification {
            label: label.into(),
            source: ClassificationSource::Rules,
            rationale: "rule".into(),
        }),
        latest_attrs: BTreeMap::new(),
    }
}

// ---------------------------------------------------------------------------

#[test]
fn picture_recovers_intact_and_is_ciphertext_at_rest() {
    let dir = scratch("pic");
    let path = dir.join("picture.sealed");

    // --- session 1: build a fused picture, hide one layer, then drop ---
    {
        let mut store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
        store
            .ensure_layer(Layer::new("maritime", "Maritime"))
            .unwrap();
        store.ensure_layer(Layer::new("air", "Air")).unwrap();
        store
            .upsert_entity(entity("v1", "maritime", "radar", "HOSTILE", 105))
            .unwrap();
        store
            .upsert_entity(entity("a7", "air", "adsb", "FRIENDLY", 110))
            .unwrap();
        store
            .set_layer_visible(&LayerId("air".into()), false)
            .unwrap();

        assert_eq!(store.picture().entity_count(), 2);
        assert_eq!(store.picture().visible_count(), 1); // air hidden
    }

    // At rest: no plaintext entity/label/layer identifiers.
    let on_disk = std::fs::read(&path).unwrap();
    for needle in [b"maritime".as_slice(), b"HOSTILE", b"radar", b"v1"] {
        assert!(
            !on_disk.windows(needle.len()).any(|w| w == needle),
            "plaintext {:?} must not be on disk",
            String::from_utf8_lossy(needle)
        );
    }

    // --- restart: reopen; entities, layers, and visibility recovered intact ---
    let store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
    let p = store.picture();
    assert_eq!(p.entity_count(), 2, "entities recovered");
    assert_eq!(p.layer_count(), 2, "layers recovered");
    assert_eq!(
        p.entity(&EntityId("v1".into()))
            .unwrap()
            .classification
            .as_ref()
            .unwrap()
            .label,
        "HOSTILE"
    );
    assert!(
        !p.is_layer_visible(&LayerId("air".into())),
        "hidden-layer toggle recovered"
    );
    assert_eq!(p.visible_count(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tampered_picture_fails_closed_on_load() {
    let dir = scratch("pictamper");
    let path = dir.join("picture.sealed");
    {
        let mut store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
        store
            .upsert_entity(entity("v1", "maritime", "radar", "HOSTILE", 105))
            .unwrap();
    }
    let mut bytes = std::fs::read(&path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    std::fs::write(&path, &bytes).unwrap();
    assert!(
        DurablePicture::open(&path, Box::new(sealer())).is_err(),
        "a tampered sealed picture must fail closed on load"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Track history sealed under the KMS: ciphertext at rest, recovered on reopen,
/// compacted to a fraction of its appended size, failing closed on tamper.
/// [FR-H-1] [NFR-PQ-2] [RC-13]
#[test]
fn track_history_is_sealed_recovered_compacted_and_fails_closed() {
    let dir = scratch("history");
    let size = |dir: &PathBuf| -> u64 {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().metadata().unwrap().len())
            .sum()
    };
    {
        let mut h = DurableHistory::open(&dir, Box::new(sealer())).unwrap();
        for batch in 0..10u64 {
            let records: Vec<HistoryRecord> = (0..100u64)
                .map(|i| HistoryRecord {
                    entity: format!("SECRET-{}", i % 20),
                    kind: "air-track".into(),
                    layer: "air".into(),
                    t: 1_000 + batch * 100 + i,
                    lat: 51.0 + i as f64 / 1000.0,
                    lon: -1.0,
                    alt_m: Some(3000.0),
                })
                .collect();
            h.record(&records, 1_000).unwrap();
        }
        assert_eq!(h.point_count(), 1000);
    }
    let appended = size(&dir);
    for entry in std::fs::read_dir(&dir).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !bytes.windows(6).any(|w| w == b"SECRET"),
            "history at rest is ciphertext"
        );
    }
    let mut h = DurableHistory::open(&dir, Box::new(sealer())).unwrap();
    assert_eq!(h.point_count(), 1000, "recovered");
    assert_eq!(h.track_count(), 20);
    let report = h.maintain(10_000, 0).unwrap();
    assert_eq!(report.segments_compacted, 1);
    let compacted = size(&dir);
    eprintln!("1000 points: {appended} bytes appended in 10 frames, {compacted} compacted");
    assert!(compacted < appended);
    assert!(compacted < 1000 * 24 + 4096, "{compacted}");
    drop(h);
    assert_eq!(
        DurableHistory::open(&dir, Box::new(sealer()))
            .unwrap()
            .point_count(),
        1000
    );

    let seg = std::fs::read_dir(&dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = std::fs::read(&seg).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    std::fs::write(&seg, &bytes).unwrap();
    assert!(
        DurableHistory::open(&dir, Box::new(sealer())).is_err(),
        "tampered history fails closed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------

fn audit_keypair() -> (VerifyingKey, SigningKey) {
    FipsSig::new(SIG_ALG).keypair(&mut SeedRng(500)).unwrap()
}

fn open_workflow(dir: &PathBuf) -> DurableWorkflow {
    let (vk, sk) = audit_keypair();
    DurableWorkflow::open(
        dir,
        Box::new(sealer()),
        Box::new(sealer()),
        Box::new(FipsSig::new(SIG_ALG)),
        sk,
        vk,
        LedgerId("heimdall-workflow".into()),
    )
    .unwrap()
}

fn verify_workflow_chain(
    entries: &[audit_writer::SealedEntry],
    provider: &CryptoProvider,
    vk: &VerifyingKey,
) {
    let vk = vk.clone();
    verify_chain(
        entries,
        provider,
        &LedgerId("heimdall-workflow".into()),
        &|_a| Some(vk.clone()),
    )
    .expect("workflow audit chain re-verifies after restart");
}

#[test]
fn in_flight_workflows_survive_restart() {
    let dir = scratch("wf");
    let (vk, _) = audit_keypair();

    // --- session 1: two proposals; one authorised (still pending execution),
    //     one still awaiting a human. Then drop the process. ---
    {
        let mut wf = open_workflow(&dir);
        wf.propose(
            "operator-1",
            ProposedAction::new("t-100", "dispatch-asset", "intercept the hostile vessel")
                .with_citations(vec!["entity:v1".into(), "feed:radar".into()]),
            "2026-07-24T00:00:00Z",
        )
        .unwrap();
        wf.propose(
            "operator-1",
            ProposedAction::new("t-200", "cue-sensor", "get a closer look"),
            "2026-07-24T00:01:00Z",
        )
        .unwrap();
        // A human authorises t-100 but it has NOT executed yet (in-flight).
        wf.authorise("t-100", "supervisor", true, "2026-07-24T00:02:00Z")
            .unwrap();

        assert_eq!(
            wf.engine().state_of("t-100"),
            Some(&WorkflowState::Authorised {
                actor: "supervisor".into()
            })
        );
        assert_eq!(
            wf.engine().state_of("t-200"),
            Some(&WorkflowState::Proposed)
        );
    }

    // --- restart: reopen; in-flight workflow state + audit lineage recovered ---
    let mut wf = open_workflow(&dir);
    assert_eq!(
        wf.engine().state_of("t-100"),
        Some(&WorkflowState::Authorised {
            actor: "supervisor".into()
        }),
        "authorised-but-unexecuted workflow recovered"
    );
    assert_eq!(
        wf.engine().state_of("t-200"),
        Some(&WorkflowState::Proposed)
    );
    // The recovered proposal keeps its cited evidence.
    let rec = wf.engine().record("t-100").unwrap();
    assert_eq!(rec.action.citations, vec!["entity:v1", "feed:radar"]);
    assert_eq!(
        rec.audit_seqs.len(),
        2,
        "propose + authorise lineage recovered"
    );

    // The recovered audit chain re-verifies under the workflow key...
    verify_workflow_chain(wf.audit_entries(), &fips_provider(), &vk);

    // ...and the workflow continues past the restart: execute the authorised action.
    let mut actuator = DispatchActuator::new();
    actuator.register("dispatch-asset", Box::new(OkEffector));
    wf.execute("operator-2", "t-100", &mut actuator, "2026-07-24T00:05:00Z")
        .unwrap();
    assert!(matches!(
        wf.engine().state_of("t-100"),
        Some(WorkflowState::Executed { .. })
    ));

    // Re-open once more to confirm the execution persisted too.
    let wf2 = open_workflow(&dir);
    assert!(matches!(
        wf2.engine().state_of("t-100"),
        Some(WorkflowState::Executed { .. })
    ));
    verify_workflow_chain(wf2.audit_entries(), &fips_provider(), &vk);

    let _ = std::fs::remove_dir_all(&dir);
}
