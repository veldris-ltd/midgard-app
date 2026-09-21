//! RC-42: stored **objects are ciphertext at rest under a KMS-wrapped key**.
//!
//! RT-10 gave the object fabric a KMS-sealed durable store; RT-42 tracks the
//! "objects sealed at rest" clause of SAD_00 §8.3 explicitly. RC-10's test already
//! proves durability, no-plaintext-leak, and tamper detection. This test pins the
//! part RC-42 names specifically — that the at-rest bytes are gated on the **KMS
//! key**, not merely obfuscated:
//!
//!   1. an object's property plaintext never appears on disk (ciphertext at rest);
//!   2. the store sealed under KMS root A **cannot** be opened under a *different*
//!      KMS root — the data key is ML-KEM-1024-wrapped to the root, so unwrapping
//!      with the wrong root fails the AEAD and the store fails closed; and
//!   3. the correct KMS root opens it and recovers the object intact.
//!
//! Together these evidence "ciphertext at rest under a KMS-wrapped key" — the
//! confidentiality is bound to KMS key custody (RT-28), not to secrecy of format.
//! [SAD_00 §8.3] [RC-42] [NFR-PQ-2] [AC-M-2]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use fabric::{DurableObjectStore, ObjectInstance, ObjectStore, Value};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use policy::{ClassificationLevel, Marking, Purpose};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

/// A distinctive sentinel that must never appear in cleartext on disk.
const SENTINEL: &str = "FERROUS-CODENAME-7Q";

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
    let dir =
        std::env::temp_dir().join(format!("fabric-rc42-{tag}-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A KMS sealer whose trust root is derived from `seed` — a distinct seed models a
/// distinct KMS root (a different ML-KEM-1024 keypair).
fn sealer_with(seed: u64, label: &str) -> KmsSealer<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, label, &mut SeedRng(seed)).unwrap();
    KmsSealer::new(Kms::new(provider, root))
}

fn dossier(id: &str) -> ObjectInstance {
    let mut properties = BTreeMap::new();
    properties.insert("codename".into(), Value::Text(SENTINEL.into()));
    properties.insert("status".into(), Value::Text("active".into()));
    ObjectInstance {
        id: id.into(),
        type_id: "dossier".into(),
        properties,
        marking: Marking {
            level: ClassificationLevel::SECRET,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

#[test]
fn objects_are_ciphertext_at_rest_under_a_kms_wrapped_key() {
    let dir = scratch("bound");
    let path = dir.join("fabric.store");

    // Leetspeak seeds ("alice seed" / "bob seed"). Written without digit
    // separators: the natural word split gives uneven groups, which trips
    // clippy::unusual_byte_groupings under -D warnings.
    const ROOT_A: u64 = 0xA11CE5EED;
    const ROOT_B: u64 = 0xB0B5EED; // a DIFFERENT KMS root

    // --- seal an object under KMS root A, then drop the process ---
    {
        let mut store = DurableObjectStore::open(
            &path,
            Box::new(sealer_with(ROOT_A, "kms-root-a")),
            "object-fabric",
        )
        .unwrap();
        store.put_object(dossier("d1"));
        assert!(store.poisoned().is_none(), "object durably committed");
    }

    // 1. Ciphertext at rest: the object's property plaintext is absent on disk.
    let on_disk = std::fs::read(&path).unwrap();
    let needle = SENTINEL.as_bytes();
    assert!(
        !on_disk.windows(needle.len()).any(|w| w == needle),
        "object property {SENTINEL:?} must be ciphertext at rest, not plaintext"
    );
    assert!(!on_disk.is_empty(), "the store actually persisted bytes");

    // 2. Bound to the KMS-wrapped key: a DIFFERENT KMS root cannot open it. The
    //    data key is ML-KEM-1024-wrapped to root A; unwrapping under root B yields
    //    a different shared secret and the AEAD fails → the store fails closed.
    let wrong_root = DurableObjectStore::open(
        &path,
        Box::new(sealer_with(ROOT_B, "kms-root-b")),
        "object-fabric",
    );
    assert!(
        wrong_root.is_err(),
        "a store sealed under one KMS root must NOT open under a different KMS root"
    );

    // 3. The correct KMS root opens it and recovers the object intact.
    let store = DurableObjectStore::open(
        &path,
        Box::new(sealer_with(ROOT_A, "kms-root-a")),
        "object-fabric",
    )
    .unwrap();
    let recovered = store
        .get_object("d1")
        .expect("object recovered under the correct KMS root");
    assert_eq!(
        recovered.properties.get("codename"),
        Some(&Value::Text(SENTINEL.into()))
    );
    assert_eq!(store.objects_of_type("dossier").len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}
