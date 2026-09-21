//! AC-C-2 — local data is AES-256-GCM under a non-exportable, ML-KEM-1024-wrapped
//! keystore key — and AC-C-13 — above-clearance data is never cached or rendered.
#![cfg(feature = "backend-fips")]

use midgard_client_keystore::{BrokeredKeystore, Keystore, SecurityFloor, SoftwareKeystore};
use midgard_client_store::{LocalStore, StoreError};
use policy::{ClassificationLevel, Clearance, Compartment, Marking};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, dest: &mut [u8]) {
        for b in dest {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (self.0 >> 33) as u8;
        }
    }
}

fn clearance(level: ClassificationLevel, comps: &[&str]) -> Clearance {
    Clearance {
        level,
        compartments: comps.iter().map(|c| Compartment(c.to_string())).collect(),
    }
}
fn marking(level: ClassificationLevel, comps: &[&str]) -> Marking {
    Marking {
        level,
        compartments: comps.iter().map(|c| Compartment(c.to_string())).collect(),
    }
}

fn open_store(clearance: Clearance) -> LocalStore<SoftwareKeystore> {
    let provider = fips_provider();
    let ks = SoftwareKeystore::generate(
        &provider,
        "device-root",
        SecurityFloor::HardwareBacked,
        &mut SeedRng(1),
    )
    .unwrap();
    LocalStore::open(provider, ks, clearance, SecurityFloor::NonExtractable).unwrap()
}

#[test]
fn round_trips_and_seals_at_rest() {
    let mut s = open_store(clearance(ClassificationLevel::SECRET, &[]));
    let plaintext = b"case notes: subject observed at 0300";
    s.put(
        "case:1",
        marking(ClassificationLevel::SECRET, &[]),
        plaintext,
        &mut SeedRng(7),
    )
    .unwrap();

    // Round-trips through the seal.
    assert_eq!(s.get("case:1").unwrap(), plaintext);

    // The blob at rest carries no plaintext and is wrapped by ML-KEM-1024.
    let blob = s.sealed("case:1").unwrap();
    assert_ne!(
        blob.ciphertext, plaintext,
        "ciphertext must not be plaintext"
    );
    assert!(!blob
        .ciphertext
        .windows(plaintext.len())
        .any(|w| w == plaintext));
    assert_eq!(
        blob.suite,
        kms::WRAP_SUITE,
        "data key must be ML-KEM-1024-wrapped"
    );
}

#[test]
fn above_clearance_is_never_cached_or_rendered() {
    // A CONFIDENTIAL user must not cache a SECRET object.
    let mut s = open_store(clearance(ClassificationLevel::CONFIDENTIAL, &[]));
    let err = s
        .put(
            "case:secret",
            marking(ClassificationLevel::SECRET, &[]),
            b"x",
            &mut SeedRng(3),
        )
        .unwrap_err();
    assert!(matches!(err, StoreError::AboveClearance));

    // Not cached …
    assert!(!s.contains("case:secret"));
    assert_eq!(s.len(), 0);
    // … and not rendered.
    assert!(matches!(s.get("case:secret"), Err(StoreError::NotFound)));
    assert!(s.visible_ids().is_empty());
}

#[test]
fn missing_compartment_is_above_clearance() {
    // Right level, missing need-to-know compartment ⇒ above clearance.
    let mut s = open_store(clearance(ClassificationLevel::SECRET, &["ALPHA"]));
    let err = s
        .put(
            "case:beta",
            marking(ClassificationLevel::SECRET, &["ALPHA", "BETA"]),
            b"x",
            &mut SeedRng(4),
        )
        .unwrap_err();
    assert!(matches!(err, StoreError::AboveClearance));
    assert!(!s.contains("case:beta"));
}

#[test]
fn dominated_objects_are_visible() {
    let mut s = open_store(clearance(ClassificationLevel::SECRET, &["ALPHA"]));
    s.put(
        "a",
        marking(ClassificationLevel::UNCLASSIFIED, &[]),
        b"1",
        &mut SeedRng(5),
    )
    .unwrap();
    s.put(
        "b",
        marking(ClassificationLevel::SECRET, &["ALPHA"]),
        b"2",
        &mut SeedRng(6),
    )
    .unwrap();
    let mut ids = s.visible_ids();
    ids.sort();
    assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn a_too_weak_floor_fails_closed() {
    let provider = fips_provider();
    let ks = SoftwareKeystore::generate(
        &provider,
        "web-root",
        SecurityFloor::NonExtractable, // a web-app-class key …
        &mut SeedRng(2),
    )
    .unwrap();
    // … cannot back a store that demands hardware custody.
    let err = LocalStore::open(
        provider,
        ks,
        clearance(ClassificationLevel::UNCLASSIFIED, &[]),
        SecurityFloor::HardwareBacked,
    )
    .err()
    .expect("must reject a too-weak floor");
    assert!(matches!(err, StoreError::FloorNotMet { .. }));
}

#[test]
fn brokered_custody_holds_no_local_key() {
    // AC-C-2's "or brokered for the extension": a brokered keystore holds no key
    // and is not a trust root, so it cannot build a LocalStore at all — privileged
    // ops must be brokered instead. This is enforced at the type level; here we
    // assert the custody contract it rests on.
    let bk = BrokeredKeystore::new("enclosure");
    assert!(!bk.holds_key());
    assert_eq!(bk.floor(), SecurityFloor::Brokered);
    let _ = &bk; // LocalStore::open(_, bk, ..) would not compile: bk is not a TrustRoot.
}
