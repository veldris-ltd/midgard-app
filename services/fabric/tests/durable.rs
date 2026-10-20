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
