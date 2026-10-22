//! RC-10: the object fabric store is durable and encrypted at rest. Given
//! committed objects and links, when the store restarts, then all objects, links,
//! and indexes are recovered; on-disk bytes are ciphertext; and queries (type
//! index, inverted-property index, adjacency) return identical results pre/post
//! restart. A tampered on-disk record is detected on load. [SAD_00 §8.3] [RC-10]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use fabric::{DurableObjectStore, LinkInstance, ObjectInstance, ObjectStore, Value};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use policy::{ClassificationLevel, Marking, Purpose};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

const ROOT_SEED: u64 = 24680;

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
        "fabric-durable-{tag}-{}-{}",
        std::process::id(),
        uniq
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Regenerating the software root with the same seed models the same KMS identity
/// being present after a restart.
fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root =
        SoftwareTrustRoot::generate(&provider, "fabric-root", &mut SeedRng(ROOT_SEED)).unwrap();
    KmsSealer::new(Kms::new(provider, root))
}

fn open(path: &PathBuf) -> DurableObjectStore {
    DurableObjectStore::open(path, Box::new(sealer()), "object-fabric").unwrap()
}

fn person(id: &str, name: &str, dept: &str, level: ClassificationLevel) -> ObjectInstance {
    let mut properties = BTreeMap::new();
    properties.insert("name".into(), Value::Text(name.into()));
    properties.insert("department".into(), Value::Text(dept.into()));
    properties.insert("clearance".into(), Value::Int(level.0 as i64));
    ObjectInstance {
        id: id.into(),
        type_id: "person".into(),
        properties,
        marking: Marking {
            level,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

fn case(id: &str, name: &str) -> ObjectInstance {
    let mut properties = BTreeMap::new();
    properties.insert("name".into(), Value::Text(name.into()));
    properties.insert("status".into(), Value::Text("open".into()));
    ObjectInstance {
        id: id.into(),
        type_id: "case".into(),
        properties,
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

/// Query the store three ways and return a stable snapshot for pre/post comparison.
fn snapshot(s: &dyn ObjectStore) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut people: Vec<String> = s
        .objects_of_type("person")
        .iter()
        .map(|o| o.id.clone())
        .collect();
    people.sort();
    let mut eng: Vec<String> = s
        .by_property("person", "department", "eng")
        .iter()
        .map(|o| o.id.clone())
        .collect();
    eng.sort();
    let mut neigh: Vec<String> = s
        .neighbors("case-1", "assigned")
        .iter()
        .map(|o| o.id.clone())
        .collect();
    neigh.sort();
    (people, eng, neigh)
}

fn seed(store: &mut DurableObjectStore) {
    store.put_object(person(
        "p1",
        "Alice",
        "eng",
        ClassificationLevel::CONFIDENTIAL,
    ));
    store.put_object(person("p2", "Bob", "eng", ClassificationLevel::SECRET));
    store.put_object(person(
        "p3",
        "Carol",
        "legal",
        ClassificationLevel::UNCLASSIFIED,
    ));
    store.put_object(case("case-1", "Operation Aurora"));
    store.put_link(LinkInstance {
        link_type: "assigned".into(),
        source_id: "case-1".into(),
        target_id: "p1".into(),
    });
    store.put_link(LinkInstance {
        link_type: "assigned".into(),
        source_id: "case-1".into(),
        target_id: "p2".into(),
    });
    // An update-in-place must re-index (p1 moves eng -> ops).
    store.put_object(person(
        "p1",
        "Alice",
        "ops",
        ClassificationLevel::CONFIDENTIAL,
    ));
}

#[test]
fn store_survives_restart_with_identical_queries() {
    let dir = scratch("ok");
    let path = dir.join("fabric.store");

    // --- session 1: commit objects + links, then drop the process ---
    let pre = {
        let mut store = open(&path);
        seed(&mut store);
        assert!(store.poisoned().is_none(), "all writes durably committed");
        snapshot(&store)
    };
    // p1 re-indexed to "ops": engineers are now just p2.
    assert_eq!(pre.0, vec!["p1", "p2", "p3"]);
    assert_eq!(
        pre.1,
        vec!["p2"],
        "inverted index reflects the update-in-place"
    );
    assert_eq!(pre.2, vec!["p1", "p2"], "adjacency recovered");

    // Encrypted at rest: no plaintext property leaks into the journal.
    let on_disk = std::fs::read(&path).unwrap();
    for needle in [
        b"Alice".as_slice(),
        b"Operation Aurora",
        b"legal",
        b"assigned",
    ] {
        assert!(
            !on_disk.windows(needle.len()).any(|w| w == needle),
            "plaintext {:?} must not appear on disk",
            String::from_utf8_lossy(needle)
        );
    }

    // --- restart: every object/link/index recovered; queries identical ---
    let store = open(&path);
    assert_eq!(
        store.get_object("p1"),
        Some(&person(
            "p1",
            "Alice",
            "ops",
            ClassificationLevel::CONFIDENTIAL
        ))
    );
    assert_eq!(
        store.get_object("case-1"),
        Some(&case("case-1", "Operation Aurora"))
    );
    let post = snapshot(&store);
    assert_eq!(
        pre, post,
        "queries return identical results pre/post restart"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tampered_on_disk_record_is_detected_on_load() {
    let dir = scratch("tamper");
    let path = dir.join("fabric.store");
    {
        let mut store = open(&path);
        seed(&mut store);
    }
    // Flip a byte inside a sealed record → AEAD authentication fails on reload.
    let mut bytes = std::fs::read(&path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    std::fs::write(&path, &bytes).unwrap();

    let reopened = DurableObjectStore::open(&path, Box::new(sealer()), "object-fabric");
    assert!(
        reopened.is_err(),
        "a tampered on-disk record must fail closed on load"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_removal_is_journalled_and_survives_restart() {
    let dir = scratch("remove");
    let path = dir.join("fabric.store");
    {
        let mut store = open(&path);
        seed(&mut store);
        assert!(store.remove_object_durable("p2").unwrap());
        assert!(!store.remove_object_durable("p2").unwrap(), "already gone");
        assert!(!store.remove_object_durable("nobody").unwrap());
        assert!(store.poisoned().is_none());
    }
    let store = open(&path);
    let (people, engineers, assigned) = snapshot(&store);
    assert_eq!(people, vec!["p1", "p3"]);
    assert!(engineers.is_empty(), "the removed object left the index");
    assert_eq!(assigned, vec!["p1"], "its link went with it");
}

#[test]
fn compaction_keeps_exactly_what_the_store_holds_and_survives_restart() {
    let dir = scratch("compact");
    let path = dir.join("fabric.store");
    let before = {
        let mut store = open(&path);
        seed(&mut store);
        // Rewrite p3 many times and remove p2: the journal outgrows the data.
        for _ in 0..50 {
            store.put_object(person(
                "p3",
                "Carol",
                "legal",
                ClassificationLevel::UNCLASSIFIED,
            ));
        }
        assert!(store.remove_object_durable("p2").unwrap());
        let size = std::fs::metadata(&path).unwrap().len();
        let (was, now) = store.compact().unwrap();
        assert_eq!(
            was,
            7 + 50 + 1,
            "the seed (4 objects, 2 links, 1 update), 50 rewrites, 1 removal"
        );
        assert_eq!(now, store.live_records());
        assert!(
            std::fs::metadata(&path).unwrap().len() < size / 5,
            "much smaller"
        );
        // Writes after compaction land in the new journal.
        store.put_object(case("case-2", "Operation Borealis"));
        assert!(store.poisoned().is_none());
        snapshot(&store)
    };
    assert!(!path.with_extension("compacting").exists());
    let store = open(&path);
    assert_eq!(snapshot(&store), before, "identical after restart");
    assert!(store.get_object("case-2").is_some());
    assert!(store.get_object("p2").is_none());
}

// --- batches: one flush for many records -------------------------------------

use audit_writer::{AuditError, Sealer};
use fabric::StoreOp;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn assigned(source: &str, target: &str) -> LinkInstance {
    LinkInstance {
        link_type: "assigned".into(),
        source_id: source.into(),
        target_id: target.into(),
    }
}

/// The ops [`seed`] performs, as one batch.
fn seed_ops() -> Vec<StoreOp> {
    vec![
        StoreOp::PutObject(person(
            "p1",
            "Alice",
            "eng",
            ClassificationLevel::CONFIDENTIAL,
        )),
        StoreOp::PutObject(person("p2", "Bob", "eng", ClassificationLevel::SECRET)),
        StoreOp::PutObject(person(
            "p3",
            "Carol",
            "legal",
            ClassificationLevel::UNCLASSIFIED,
        )),
        StoreOp::PutObject(case("case-1", "Operation Aurora")),
        StoreOp::PutLink(assigned("case-1", "p1")),
        StoreOp::PutLink(assigned("case-1", "p2")),
        StoreOp::PutObject(person(
            "p1",
            "Alice",
            "ops",
            ClassificationLevel::CONFIDENTIAL,
        )),
    ]
}

/// The (offset, length) of every frame in a journal file.
fn frames(path: &PathBuf) -> Vec<(usize, usize)> {
    let bytes = std::fs::read(path).unwrap();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        out.push((pos, 4 + len));
        pos += 4 + len;
    }
    out
}

#[test]
fn a_batch_applies_every_op_with_one_flush_and_survives_restart() {
    let dir = scratch("batch");
    let path = dir.join("fabric.store");
    let pre = {
        let mut store = open(&path);
        let flushes = store.flushes();
        let mut ops = seed_ops();
        ops.push(StoreOp::RemoveObject("p3".into()));
        // Nothing to remove: not journalled, as with a single removal.
        ops.push(StoreOp::RemoveObject("p3".into()));
        ops.push(StoreOp::RemoveObject("nobody".into()));
        assert_eq!(store.apply_batch(ops).unwrap(), 8, "records journalled");
        assert_eq!(store.flushes(), flushes + 1, "one flush for the batch");
        assert_eq!(store.journal_records(), 8);
        assert!(store.get_object("p3").is_none());
        snapshot(&store)
    };
    assert_eq!(pre.0, vec!["p1", "p2"]);
    assert_eq!(pre.1, vec!["p2"], "the in-batch update re-indexed p1");
    assert_eq!(pre.2, vec!["p1", "p2"]);
    let store = open(&path);
    assert_eq!(snapshot(&store), pre, "identical after restart");
    assert!(store.get_object("p3").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_batch_writes_and_flushes_nothing() {
    let dir = scratch("batch-empty");
    let path = dir.join("fabric.store");
    let mut store = open(&path);
    let flushes = store.flushes();
    assert_eq!(store.apply_batch(Vec::new()).unwrap(), 0);
    assert_eq!(
        store
            .apply_batch(vec![StoreOp::RemoveObject("nobody".into())])
            .unwrap(),
        0
    );
    assert_eq!(store.flushes(), flushes);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn single_writes_still_flush_each_record() {
    let dir = scratch("single");
    let path = dir.join("fabric.store");
    let mut store = open(&path);
    let flushes = store.flushes();
    seed(&mut store);
    assert!(store.remove_object_durable("p3").unwrap());
    assert!(!store.remove_object_durable("p3").unwrap());
    assert_eq!(
        store.flushes(),
        flushes + 8,
        "seven seed writes and one removal"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_part_way_through_a_batch_recovers_to_a_prefix_of_it() {
    let dir = scratch("batch-crash");
    let path = dir.join("fabric.store");
    {
        let mut store = open(&path);
        store.put_object(case("case-0", "Before"));
        store.apply_batch(seed_ops()).unwrap();
    }
    let whole = std::fs::read(&path).unwrap();
    let frames = frames(&path);
    assert_eq!(frames.len(), 8, "one frame per record, as before");

    // The process died after the first four of the batch's seven records reached disk.
    let (at, _) = frames[1 + 4];
    std::fs::write(&path, &whole[..at]).unwrap();
    let store = open(&path);
    assert!(
        store.get_object("case-0").is_some(),
        "the earlier write survives"
    );
    let (people, engineers, links) = snapshot(&store);
    assert_eq!(people, vec!["p1", "p2", "p3"]);
    assert_eq!(
        engineers,
        vec!["p1", "p2"],
        "p1's in-batch update never landed"
    );
    assert!(links.is_empty(), "nor did the links after it");
    assert_eq!(store.journal_records(), 5);
    drop(store);

    // Torn inside a frame: open fails closed, exactly as a torn single write does.
    let (at, len) = frames[1 + 4];
    std::fs::write(&path, &whole[..at + len / 2]).unwrap();
    assert!(DurableObjectStore::open(&path, Box::new(sealer()), "object-fabric").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Seals until its budget runs out, then fails.
struct Failing {
    inner: KmsSealer<SoftwareTrustRoot>,
    budget: Arc<AtomicUsize>,
}
impl Sealer for Failing {
    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
        if self.budget.fetch_sub(1, Ordering::SeqCst) == 0 {
            self.budget.store(0, Ordering::SeqCst);
            return Err(AuditError::Store("out of budget".into()));
        }
        self.inner.seal(aad, plaintext)
    }
    fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
        self.inner.unseal(aad, sealed)
    }
}

#[test]
fn a_failed_batch_applies_nothing_and_the_store_carries_on() {
    let dir = scratch("batch-fail");
    let path = dir.join("fabric.store");
    let budget = Arc::new(AtomicUsize::new(1 + 3));
    {
        let mut store = DurableObjectStore::open(
            &path,
            Box::new(Failing {
                inner: sealer(),
                budget: budget.clone(),
            }),
            "object-fabric",
        )
        .unwrap();
        store.put_object(case("case-0", "Before"));
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(
            store.apply_batch(seed_ops()).is_err(),
            "the fourth seal fails"
        );
        assert!(
            store.get_object("p1").is_none(),
            "nothing applied to memory"
        );
        assert_eq!(store.journal_records(), 1);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), len, "nor to disk");
        budget.store(usize::MAX / 2, Ordering::SeqCst);
        store.apply_batch(seed_ops()).unwrap();
    }
    let store = open(&path);
    assert!(store.get_object("case-0").is_some());
    assert_eq!(snapshot(&store).2, vec!["p1", "p2"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_store_trait_applies_a_batch_with_one_flush() {
    // How the fabric applies an action's effects: through the trait, in one go.
    let dir = scratch("batch-trait");
    let path = dir.join("fabric.store");
    let mut store = open(&path);
    let flushes = store.flushes();
    {
        let dynamic: &mut dyn ObjectStore = &mut store;
        dynamic.apply_all(seed_ops());
    }
    assert_eq!(store.flushes(), flushes + 1);
    assert!(store.poisoned().is_none());
    assert_eq!(snapshot(&store).2, vec!["p1", "p2"]);

    // The in-memory store applies the same ops in the same order.
    let mut memory = fabric::InMemoryStore::new();
    memory.apply_all(seed_ops());
    assert_eq!(snapshot(&memory), snapshot(&store));
    let _ = std::fs::remove_dir_all(&dir);
}
