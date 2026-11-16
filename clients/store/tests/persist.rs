//! The sealed cache survives a relaunch, holds nothing readable at rest, drops what
//! the new clearance may not hold, is bound to the scope (persona) that wrote it,
//! and fails closed — loading nothing — on any change to the file. [AC-C-2]
//! [AC-C-13] [FR-C-2]
#![cfg(feature = "backend-fips")]

use midgard_client_keystore::sealed_root::{RootSealer, SealedRootKeystore};
use midgard_client_keystore::{KeystoreError, SecurityFloor};
use midgard_client_store::{LocalStore, StoreError};
use policy::{ClassificationLevel as L, Clearance, Compartment, Marking};
use pqc_agility::backend::fips::provider;
use pqc_agility::{Aes256GcmAead, OsRng, Rng, SecretBytes, GCM_NONCE_LEN};

struct TestSealer([u8; 32]);
impl RootSealer for TestSealer {
    fn seal(&self, data_key: &SecretBytes) -> Result<Vec<u8>, KeystoreError> {
        let mut nonce = [0u8; GCM_NONCE_LEN];
        OsRng.fill(&mut nonce);
        let mut out = nonce.to_vec();
        out.extend(
            Aes256GcmAead::new(&self.0)
                .seal(&nonce, b"", data_key.expose())
                .unwrap(),
        );
        Ok(out)
    }
    fn unseal(&self, sealed: &[u8]) -> Result<SecretBytes, KeystoreError> {
        let nonce: [u8; GCM_NONCE_LEN] = sealed[..GCM_NONCE_LEN].try_into().unwrap();
        Aes256GcmAead::new(&self.0)
            .open(&nonce, b"", &sealed[GCM_NONCE_LEN..])
            .map(SecretBytes::new)
            .map_err(|_| KeystoreError::Unseal)
    }
    fn hardware_backed(&self) -> bool {
        false
    }
}

fn cl(level: L) -> Clearance {
    Clearance {
        level,
        compartments: Default::default(),
    }
}
fn mk(level: L) -> Marking {
    Marking {
        level,
        compartments: Default::default(),
    }
}

type Store = LocalStore<SealedRootKeystore>;

fn root(sealer: &TestSealer) -> Vec<u8> {
    SealedRootKeystore::create(&provider(), "device-root", sealer, &mut OsRng)
        .unwrap()
        .1
}

fn open(root: &[u8], sealer: &TestSealer, clearance: Clearance, scope: &str) -> Store {
    let ks = SealedRootKeystore::open(root, sealer).unwrap();
    LocalStore::open_scoped(
        provider(),
        ks,
        clearance,
        SecurityFloor::NonExtractable,
        scope,
    )
    .unwrap()
}

/// A store holding a few rows, its exported file, and what to reopen it with.
fn written() -> (TestSealer, Vec<u8>, Vec<u8>) {
    let sealer = TestSealer([1; 32]);
    let root = root(&sealer);
    let writer = Clearance {
        level: L::SECRET,
        compartments: [Compartment("OPS-NORTH".into())].into(),
    };
    let mut s = open(&root, &sealer, writer, "persona-one");
    s.put(
        "row-alpha",
        mk(L::UNCLASSIFIED),
        b"open row text",
        &mut OsRng,
    )
    .unwrap();
    let ops = Marking {
        level: L::CONFIDENTIAL,
        compartments: [Compartment("OPS-NORTH".into())].into(),
    };
    s.put("row-bravo", ops, b"confidential row text", &mut OsRng)
        .unwrap();
    s.put("row-charlie", mk(L::SECRET), b"secret row text", &mut OsRng)
        .unwrap();
    let file = s.export_sealed(&mut OsRng).unwrap();
    (sealer, root, file)
}

#[test]
fn the_cache_round_trips_and_drops_above_clearance_rows() {
    let (sealer, root, file) = written();
    let mut same = open(&root, &sealer, cl(L::SECRET), "persona-one");
    assert_eq!(same.import_sealed(&file).unwrap(), (2, 1));
    assert_eq!(same.get("row-alpha").unwrap(), b"open row text");
    assert_eq!(same.get("row-charlie").unwrap(), b"secret row text");
    // row-bravo is in a compartment this clearance does not hold.
    assert!(matches!(same.get("row-bravo"), Err(StoreError::NotFound)));

    let mut lower = open(&root, &sealer, cl(L::CONFIDENTIAL), "persona-one");
    assert_eq!(lower.import_sealed(&file).unwrap(), (1, 2));
    assert_eq!(lower.get("row-alpha").unwrap(), b"open row text");
    assert!(matches!(
        lower.get("row-charlie"),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn the_file_holds_no_plaintext_ids_markings_or_scope() {
    let (_, _, file) = written();
    for needle in [
        &b"open row text"[..],
        b"secret row text",
        b"row-alpha",
        b"row-bravo",
        b"row-charlie",
        b"OPS-NORTH",
        b"persona-one",
        b"MGCACHE",
    ] {
        assert!(
            !file.windows(needle.len()).any(|w| w == needle),
            "{} is readable in the file",
            String::from_utf8_lossy(needle)
        );
    }
}

#[test]
fn a_file_sealed_to_another_root_is_refused_and_nothing_is_loaded() {
    let (_, _, file) = written();
    // Same key id, another root: exactly the case where only AES-GCM can tell.
    let other = TestSealer([2; 32]);
    let mut t = open(&root(&other), &other, cl(L::SECRET), "persona-one");
    assert!(t.import_sealed(&file).is_err());
    assert!(t.is_empty());
}

#[test]
fn a_file_written_for_another_scope_is_refused() {
    let (sealer, root, file) = written();
    let mut t = open(&root, &sealer, cl(L::SECRET), "persona-two");
    assert!(t.import_sealed(&file).is_err());
    assert!(t.is_empty());
}

#[test]
fn any_flipped_byte_or_truncation_loads_nothing() {
    let (sealer, root, file) = written();
    let mut at: Vec<usize> = (0..48).collect();
    at.extend((48..file.len()).step_by(61));
    at.extend(file.len() - 20..file.len());
    for i in at {
        let mut bad = file.clone();
        bad[i] ^= 0x80;
        let mut t = open(&root, &sealer, cl(L::SECRET), "persona-one");
        t.put("already", mk(L::UNCLASSIFIED), b"kept", &mut OsRng)
            .unwrap();
        assert!(t.import_sealed(&bad).is_err(), "byte {i} flipped accepted");
        // A refused file changes nothing: the store is as it was.
        assert_eq!(t.len(), 1);
        assert_eq!(t.get("already").unwrap(), b"kept");
    }
    for n in [0, 3, 4, 20, file.len() / 2, file.len() - 1] {
        let mut t = open(&root, &sealer, cl(L::SECRET), "persona-one");
        assert!(t.import_sealed(&file[..n]).is_err(), "truncated to {n}");
        assert!(t.is_empty());
    }
}

#[test]
fn an_import_replaces_what_was_cached() {
    let (sealer, root, file) = written();
    let mut t = open(&root, &sealer, cl(L::SECRET), "persona-one");
    t.put("stale", mk(L::UNCLASSIFIED), b"old", &mut OsRng)
        .unwrap();
    t.import_sealed(&file).unwrap();
    assert!(!t.contains("stale"));
    assert_eq!(t.len(), 2);
}

#[test]
fn a_change_of_scope_drops_every_row_and_a_change_of_clearance_drops_the_rest() {
    let sealer = TestSealer([1; 32]);
    let root = root(&sealer);
    let mut s = open(&root, &sealer, cl(L::SECRET), "persona-one");
    s.put("a", mk(L::UNCLASSIFIED), b"x", &mut OsRng).unwrap();
    s.put("b", mk(L::SECRET), b"y", &mut OsRng).unwrap();
    // Same scope, lower clearance: only what it does not dominate goes.
    let (s, dropped) = s.rescope("persona-one", cl(L::CONFIDENTIAL));
    assert_eq!(dropped, 1);
    assert_eq!(s.get("a").unwrap(), b"x");
    // Another scope, even with a higher clearance: nothing of the first is kept.
    let (s, dropped) = s.rescope("persona-two", cl(L::TOP_SECRET));
    assert_eq!(dropped, 1);
    assert!(s.is_empty());
    assert_eq!(s.scope(), "persona-two");
}
