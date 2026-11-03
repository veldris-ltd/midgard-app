//! RC-13: the fused picture and in-flight workflows survive a Heimdall restart —
//! recovered intact, with at-rest bytes as ciphertext, failing closed on tamper.
//! The picture persists as a sealed journal frame per change, compacted into the
//! snapshot, and a picture sealed whole by an earlier build still opens.
//! [FR-H-1] [FR-H-5] [RC-13] [VLD-ANB-MIDGARD-0017]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use audit_writer::{verify_chain, LedgerId, Sealer};
use heimdall_picture::{
    Classification, ClassificationSource, EntityId, Layer, LayerId, Picture, PictureDelta, Track,
    TrackedEntity,
};
use heimdall_store::{DurablePicture, DurableWorkflow};
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
            last_received: last_seen,
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

    // At rest: no plaintext entity/label/layer identifiers, in the journal or in a
    // snapshot.
    let mut on_disk = std::fs::read(dir.join("picture.journal")).unwrap();
    on_disk.extend(std::fs::read(&path).unwrap_or_default());
    assert!(!on_disk.is_empty());
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
    let journal = dir.join("picture.journal");
    let flip = |file: &PathBuf| {
        let mut bytes = std::fs::read(file).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x01;
        std::fs::write(file, &bytes).unwrap();
    };
    let pristine = std::fs::read(&journal).unwrap();
    flip(&journal);
    assert!(
        DurablePicture::open(&path, Box::new(sealer())).is_err(),
        "a tampered journal must fail closed on load"
    );

    // And a tampered snapshot, once the journal is compacted into it.
    std::fs::write(&journal, pristine).unwrap();
    DurablePicture::open(&path, Box::new(sealer()))
        .unwrap()
        .compact()
        .unwrap();
    flip(&path);
    assert!(
        DurablePicture::open(&path, Box::new(sealer())).is_err(),
        "a tampered sealed picture must fail closed on load"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn file_len(path: &PathBuf) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

#[test]
fn each_upsert_appends_one_frame_and_the_file_grows_by_one_entity_not_the_picture() {
    // [VLD-ANB-MIDGARD-0017] [AC-44.2]
    let dir = scratch("picgrowth");
    let path = dir.join("picture.sealed");
    let journal = dir.join("picture.journal");
    let mut store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
    store
        .ensure_layer(Layer::new("maritime", "Maritime"))
        .unwrap();

    let mut growth = Vec::new();
    for i in 0..1_000 {
        let before = file_len(&journal);
        store
            .upsert_entity(entity(
                &format!("v{i:04}"),
                "maritime",
                "radar",
                "HOSTILE",
                105,
            ))
            .unwrap();
        growth.push(file_len(&journal) - before);
    }

    assert_eq!(store.picture().entity_count(), 1_000);
    assert_eq!(store.frames_since_compaction(), 1_001, "a frame per change");
    let (tenth, thousandth) = (growth[9], growth[999]);
    assert!(tenth > 0);
    assert!(
        thousandth <= 2 * tenth,
        "the 1,000th upsert added {thousandth} bytes, the 10th {tenth}"
    );
    assert!(
        !path.exists(),
        "no change rewrote the whole picture into the snapshot"
    );

    drop(store);
    let store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
    assert_eq!(store.picture().entity_count(), 1_000, "the journal replays");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compaction_shrinks_the_journal_and_the_picture_survives_restart() {
    // [VLD-ANB-MIDGARD-0017] [AC-44.2]
    let dir = scratch("piccompact");
    let path = dir.join("picture.sealed");
    let journal = dir.join("picture.journal");
    let mut store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
    store.ensure_layer(Layer::new("air", "Air")).unwrap();
    store
        .set_layer_visible(&LayerId("air".into()), false)
        .unwrap();

    // Ten tracks updated over and over: the journal is compacted by itself once it
    // holds more than 2 x 10 + 1,000 frames, and never grows past that.
    let mut peak = 0;
    for round in 0..150u64 {
        for i in 0..10 {
            store
                .upsert_entity(entity(
                    &format!("a{i}"),
                    "air",
                    "adsb",
                    "FRIENDLY",
                    200 + round,
                ))
                .unwrap();
            assert!(store.frames_since_compaction() <= 2 * 10 + 1_000);
            peak = peak.max(file_len(&journal));
        }
    }
    assert!(path.exists(), "compaction wrote the snapshot");
    assert!(
        file_len(&journal) < peak,
        "compaction shrank the journal from its peak of {peak} bytes"
    );

    store.compact().unwrap();
    assert_eq!(store.frames_since_compaction(), 0);
    assert_eq!(
        file_len(&journal),
        0,
        "an explicit compaction empties the journal"
    );
    assert!(!dir.join("picture.journal.old").exists());

    drop(store);
    let store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
    let p = store.picture();
    assert_eq!(p.entity_count(), 10);
    assert_eq!(
        p.entity(&EntityId("a3".into())).unwrap().last_seen(),
        349,
        "the latest state of each track survives"
    );
    assert!(!p.is_layer_visible(&LayerId("air".into())));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_removed_entity_stays_removed_after_restart() {
    // [VLD-ANB-MIDGARD-0017] [AC-44.2]
    let dir = scratch("picremove");
    let path = dir.join("picture.sealed");
    {
        let mut store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
        for id in ["v1", "v2", "v3"] {
            store
                .upsert_entity(entity(id, "maritime", "radar", "HOSTILE", 105))
                .unwrap();
        }
        // v1 is in the snapshot when it is removed; v2 only in the journal.
        store.compact().unwrap();
        store
            .upsert_entity(entity("v2", "maritime", "radar", "SUSPECT", 106))
            .unwrap();
        assert!(store
            .remove_entity(&EntityId("v1".into()))
            .unwrap()
            .is_some());
        assert!(store
            .remove_entity(&EntityId("v2".into()))
            .unwrap()
            .is_some());
        assert!(store
            .remove_entity(&EntityId("v9".into()))
            .unwrap()
            .is_none());
    }
    let ids = |store: &DurablePicture| -> Vec<String> {
        store.picture().entities().map(|e| e.id.0.clone()).collect()
    };

    let mut store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
    assert_eq!(ids(&store), ["v3"]);
    store.compact().unwrap();
    drop(store);
    let store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
    assert_eq!(ids(&store), ["v3"], "and after the removal is compacted");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The whole-picture blob earlier builds resealed on every change: the picture's
/// delta snapshot, then the hidden layers, sealed under the picture AAD.
fn whole_picture_blob(picture: &Picture) -> Vec<u8> {
    let delta = PictureDelta::snapshot(picture).canonical_bytes();
    let hidden: Vec<&LayerId> = picture
        .layers()
        .map(|l| &l.id)
        .filter(|id| !picture.is_layer_visible(id))
        .collect();
    let mut plaintext = (delta.len() as u32).to_le_bytes().to_vec();
    plaintext.extend_from_slice(&delta);
    plaintext.extend_from_slice(&(hidden.len() as u32).to_le_bytes());
    for id in hidden {
        plaintext.extend_from_slice(&(id.0.len() as u32).to_le_bytes());
        plaintext.extend_from_slice(id.0.as_bytes());
    }
    sealer()
        .seal(b"midgard-heimdall-picture-v1", &plaintext)
        .unwrap()
}

#[test]
fn a_picture_sealed_whole_by_an_earlier_build_opens_and_compacts_back_to_that_format() {
    // [VLD-ANB-MIDGARD-0017] [RC-13]
    let dir = scratch("picmigrate");
    let path = dir.join("picture.sealed");
    let mut earlier = Picture::new();
    earlier.ensure_layer(Layer::new("maritime", "Maritime"));
    earlier.ensure_layer(Layer::new("air", "Air"));
    earlier.upsert_entity(entity("v1", "maritime", "radar", "HOSTILE", 105));
    earlier.upsert_entity(entity("a7", "air", "adsb", "FRIENDLY", 110));
    earlier.set_layer_visible(&LayerId("air".into()), false);
    std::fs::write(&path, whole_picture_blob(&earlier)).unwrap();
    assert!(!dir.join("picture.journal").exists());

    // The upgrade: the old file is the snapshot and the journal starts empty.
    {
        let mut store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
        assert_eq!(
            store.picture(),
            &earlier,
            "the earlier picture opens intact"
        );
        assert_eq!(store.frames_since_compaction(), 0);
        store
            .upsert_entity(entity("v2", "maritime", "radar", "SUSPECT", 120))
            .unwrap();
    }
    let mut store = DurablePicture::open(&path, Box::new(sealer())).unwrap();
    assert_eq!(store.picture().entity_count(), 3);
    assert!(!store.picture().is_layer_visible(&LayerId("air".into())));

    // A rollback: once compacted, the snapshot is in the earlier build's format.
    store.compact().unwrap();
    let expected = store.picture().clone();
    drop(store);
    let plaintext = sealer()
        .unseal(
            b"midgard-heimdall-picture-v1",
            &std::fs::read(&path).unwrap(),
        )
        .unwrap();
    let len = u32::from_le_bytes(plaintext[..4].try_into().unwrap()) as usize;
    let delta = PictureDelta::from_canonical_bytes(&plaintext[4..4 + len]).unwrap();
    assert_eq!(delta, PictureDelta::snapshot(&expected));
    let hidden = &plaintext[4 + len..];
    assert_eq!(
        hidden,
        [&1u32.to_le_bytes()[..], &3u32.to_le_bytes(), b"air"].concat()
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
