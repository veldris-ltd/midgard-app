//! A timing probe, not a correctness test: how long the durable store takes to take in
//! a sync-sized batch of objects, and where the time goes (sealing vs flushing). Run it
//! with `cargo test --release -p fabric --test durable_bench -- --ignored --nocapture`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

use audit_writer::{AuditError, Sealer};
use fabric::{DurableObjectStore, ObjectInstance, StoreOp, Value};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use policy::{ClassificationLevel, Marking, Purpose};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

const N: usize = 500;

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (self.0 >> 33) as u8;
        }
    }
}

fn kms_sealer() -> KmsSealer<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "bench-root", &mut SeedRng(7)).unwrap();
    KmsSealer::new(Kms::new(provider, root))
}

/// Seals nothing: isolates the cost of writing and flushing the journal.
struct Plain;
impl Sealer for Plain {
    fn seal(&self, _aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
        Ok(plaintext.to_vec())
    }
    fn unseal(&self, _aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
        Ok(sealed.to_vec())
    }
}

fn track(i: usize) -> ObjectInstance {
    let mut properties = BTreeMap::new();
    properties.insert("name".into(), Value::Text(format!("Track {i}")));
    properties.insert(
        "last_seen".into(),
        Value::Timestamp(1_700_000_000 + i as u64),
    );
    properties.insert("lat".into(), Value::Float(51.0 + i as f64 / 1000.0));
    ObjectInstance {
        id: format!("track/{i}"),
        type_id: "track".into(),
        properties,
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("operations".into())].into_iter().collect(),
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fabric-bench-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn per_object(tag: &str, sealer: Box<dyn Sealer>) -> f64 {
    let dir = scratch(tag);
    let mut store = DurableObjectStore::open(dir.join("fabric.store"), sealer, "bench").unwrap();
    let t = Instant::now();
    for i in 0..N {
        store.put_object_durable(track(i)).unwrap();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
    ms
}

fn one_batch(tag: &str, sealer: Box<dyn Sealer>) -> f64 {
    let dir = scratch(tag);
    let mut store = DurableObjectStore::open(dir.join("fabric.store"), sealer, "bench").unwrap();
    let ops = (0..N).map(|i| StoreOp::PutObject(track(i))).collect();
    let t = Instant::now();
    store.apply_batch(ops).unwrap();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(store.flushes(), 1);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
    ms
}

#[test]
#[ignore = "timing probe; run by hand"]
fn time_a_sync_sized_batch() {
    let sealer = kms_sealer();
    let t = Instant::now();
    for i in 0..N {
        sealer.seal(b"aad", &[i as u8; 200]).unwrap();
    }
    let seal_ms = t.elapsed().as_secs_f64() * 1000.0;
    let plain = per_object("plain", Box::new(Plain));
    let sealed = per_object("kms", Box::new(kms_sealer()));
    println!("{N} objects, one at a time:");
    println!("  sealing alone               {seal_ms:8.1} ms");
    println!("  journal + flush, no sealing {plain:8.1} ms");
    println!("  journal + flush + sealing   {sealed:8.1} ms");
    let batch = one_batch("batch", Box::new(kms_sealer()));
    println!("{N} objects, one batch:");
    println!("  journal + flush + sealing   {batch:8.1} ms");
}
