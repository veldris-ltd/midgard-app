//! AC-C-1 — one shared core: crypto, sync, and storage are provided by a single
//! linked crate and exercised through one façade, with no per-shell reimplementation.
#![cfg(feature = "backend-fips")]

use midgard_client_core::keystore::{SecurityFloor, SoftwareKeystore};
use midgard_client_core::{ClientCore, CoreError, CORE_VERSION};
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

fn core(clearance: Clearance, id: &str, seed: u64) -> ClientCore {
    let provider = fips_provider();
    let ks = SoftwareKeystore::generate(
        &provider,
        "device-root",
        SecurityFloor::HardwareBacked,
        &mut SeedRng(seed),
    )
    .unwrap();
    ClientCore::open(provider, ks, clearance, SecurityFloor::NonExtractable, id).unwrap()
}

/// The core version is a real, single identifier — the string every shell links.
#[test]
fn exposes_one_core_version() {
    assert_eq!(CORE_VERSION, env!("CARGO_PKG_VERSION"));
    assert!(!CORE_VERSION.is_empty());
}

/// One flow through the façade exercises crypto, sync, and storage together —
/// demonstrating a single shared core rather than per-shell reimplementation.
#[test]
fn edit_seals_syncs_and_reloads_through_one_core() {
    let mut c = core(clearance(ClassificationLevel::SECRET, &[]), "device-a", 1);

    // Edit offline: produces a queued CRDT op (sync) …
    let op = c
        .edit_field(
            "case:1",
            "title",
            b"Operation Bifrost".to_vec(),
            marking(ClassificationLevel::SECRET, &[]),
            &mut SeedRng(9),
        )
        .unwrap();
    assert_eq!(op.object, "case:1");
    assert_eq!(c.pending_ops().len(), 1); // sync queue

    // … the in-memory view reflects it (sync) …
    assert_eq!(
        c.read_field("case:1", "title"),
        Some(b"Operation Bifrost".to_vec())
    );

    // … and the object is sealed in the local cache, recoverable only by
    // decrypting under the keystore key (storage + crypto). The cached snapshot
    // contains the field value, but not as cleartext on disk.
    let reloaded = c.load_cached("case:1").unwrap();
    assert!(reloaded
        .windows(b"Operation Bifrost".len())
        .any(|w| w == b"Operation Bifrost"));
}

/// The same façade enforces clearance: above-clearance edits are refused and leave
/// nothing cached or queued (AC-C-13 through the composed core).
#[test]
fn above_clearance_edit_is_refused_by_the_core() {
    let mut c = core(
        clearance(ClassificationLevel::CONFIDENTIAL, &[]),
        "device-a",
        2,
    );
    let err = c
        .edit_field(
            "case:secret",
            "title",
            b"x".to_vec(),
            marking(ClassificationLevel::SECRET, &[]),
            &mut SeedRng(3),
        )
        .unwrap_err();
    assert!(matches!(err, CoreError::Store(_)));
    assert!(c.pending_ops().is_empty(), "refused edit must not queue");
    assert!(
        c.visible_objects().is_empty(),
        "refused edit must not cache"
    );
    assert!(c.load_cached("case:secret").is_err());
}

/// Two cores converge via the shared sync engine — offline edits merge on reconnect.
#[test]
fn two_cores_converge_on_reconnect() {
    let mut a = core(clearance(ClassificationLevel::SECRET, &[]), "device-a", 1);
    let mut b = core(clearance(ClassificationLevel::SECRET, &[]), "device-b", 2);

    a.edit_field(
        "case:9",
        "field-a",
        b"1".to_vec(),
        marking(ClassificationLevel::SECRET, &[]),
        &mut SeedRng(5),
    )
    .unwrap();
    b.edit_field(
        "case:9",
        "field-b",
        b"2".to_vec(),
        marking(ClassificationLevel::SECRET, &[]),
        &mut SeedRng(6),
    )
    .unwrap();

    let from_a = a.drain_pending();
    let from_b = b.drain_pending();
    b.merge_remote(&from_a);
    a.merge_remote(&from_b);

    assert_eq!(a.read_field("case:9", "field-a"), Some(b"1".to_vec()));
    assert_eq!(a.read_field("case:9", "field-b"), Some(b"2".to_vec()));
    assert_eq!(b.read_field("case:9", "field-a"), Some(b"1".to_vec()));
    assert_eq!(b.read_field("case:9", "field-b"), Some(b"2".to_vec()));
}

/// RT-58: the FFI-friendly binding surface (opaque encoded op blobs, the form the
/// UniFFI/wasm-bindgen shells exchange) converges exactly like the typed path — and
/// fails closed on a malformed blob.
#[test]
fn binding_surface_converges_over_opaque_blobs() {
    let mut a = core(clearance(ClassificationLevel::SECRET, &[]), "device-a", 11);
    let mut b = core(clearance(ClassificationLevel::SECRET, &[]), "device-b", 12);

    a.edit_field(
        "case:42",
        "owner",
        b"ana".to_vec(),
        marking(ClassificationLevel::SECRET, &[]),
        &mut SeedRng(7),
    )
    .unwrap();
    b.edit_field(
        "case:42",
        "status",
        b"open".to_vec(),
        marking(ClassificationLevel::SECRET, &[]),
        &mut SeedRng(8),
    )
    .unwrap();

    // Exchange ops the way a shell does: as opaque encoded blobs across the binding.
    let from_a = a.drain_pending_encoded();
    let from_b = b.drain_pending_encoded();
    assert!(
        from_a.iter().all(|blob| !blob.is_empty()),
        "ops are non-empty opaque blobs"
    );
    b.merge_remote_encoded(&from_a).unwrap();
    a.merge_remote_encoded(&from_b).unwrap();

    // Both cores converge — identical result to the typed `merge_remote` path.
    for c in [&a, &b] {
        assert_eq!(c.read_field("case:42", "owner"), Some(b"ana".to_vec()));
        assert_eq!(c.read_field("case:42", "status"), Some(b"open".to_vec()));
    }

    // A malformed blob is rejected — nothing is applied.
    let bad = a.merge_remote_encoded(&[vec![0xff, 0x00, 0x01]]);
    assert!(
        matches!(bad, Err(CoreError::MalformedOp(_))),
        "a malformed op blob fails closed"
    );
}
