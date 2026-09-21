//! # midgard-client-store — the local encrypted, policy-scoped object cache
//!
//! Every client holds only an encrypted, policy-scoped slice of the fabric. This
//! crate is that slice. Two invariants, both enforced here so no shell can bypass
//! them (`[KDD-1]`):
//!
//! 1. **Sealed at rest.** Each cached object is AES-256-GCM-encrypted under a data
//!    key wrapped by ML-KEM-1024, with the **platform keystore as the trust
//!    root** — the wrapped key is unwrapped only inside non-exportable custody.
//!    The store owns no key material; it holds a [`kms::Kms`] over a keystore, so a
//!    hardware root drops in unchanged. The object id and its marking are bound in
//!    as AAD, so a sealed blob cannot be swapped between objects. `[AC-C-2]`
//!    `[NFR-PQ-2]`
//! 2. **Never above clearance.** A [`put`](LocalStore::put) whose marking is not
//!    dominated by the session clearance is *refused and never stored*, and
//!    [`get`](LocalStore::get) re-checks dominance before returning — so
//!    above-clearance data is neither cached nor rendered client-side. Marking
//!    enforcement remains authoritative in the enclosure; this is the client-side
//!    floor. `[AC-C-13]` `[NFR-SEC-2]`

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use kms::{Kms, SealedBlob, TrustRoot};
use midgard_client_keystore::{Keystore, SecurityFloor};
use policy::{dominates, Clearance, Marking};
use pqc_agility::{CryptoProvider, Rng};

mod error;
pub use error::StoreError;

/// One cached object: its marking and the sealed bytes at rest (no plaintext).
#[derive(Debug, Clone)]
struct CachedObject {
    marking: Marking,
    sealed: SealedBlob,
}

/// A local, encrypted, clearance-scoped object cache backed by a keystore root.
///
/// Generic over the trust root so a hardware keystore substitutes for the software
/// one with no code change. Constructed only from a **key-holding** keystore that
/// meets a minimum floor; the extension's brokered custody cannot build one (it is
/// not a [`TrustRoot`]), which is exactly the least-privilege split KDD-2 requires.
pub struct LocalStore<R: TrustRoot + Keystore> {
    kms: Kms<R>,
    clearance: Clearance,
    objects: BTreeMap<String, CachedObject>,
}

impl<R: TrustRoot + Keystore> LocalStore<R> {
    /// Open a store over `keystore`, requiring it to hold a key and meet
    /// `min_floor`. Fails closed if custody is too weak — the client refuses to
    /// cache under an inadequate key rather than degrade silently. `[FR-C-2]`
    pub fn open(
        provider: CryptoProvider,
        keystore: R,
        clearance: Clearance,
        min_floor: SecurityFloor,
    ) -> Result<Self, StoreError> {
        if !keystore.holds_key() {
            return Err(StoreError::NoLocalKey);
        }
        if keystore.floor() < min_floor {
            return Err(StoreError::FloorNotMet {
                required: min_floor,
                actual: keystore.floor(),
            });
        }
        Ok(Self {
            kms: Kms::new(provider, keystore),
            clearance,
            objects: BTreeMap::new(),
        })
    }

    /// Cache `plaintext` for `id` at `marking`. Refused (and nothing stored) if the
    /// session clearance does not dominate the marking. `[AC-C-13]`
    pub fn put(
        &mut self,
        id: &str,
        marking: Marking,
        plaintext: &[u8],
        rng: &mut dyn Rng,
    ) -> Result<(), StoreError> {
        if !dominates(&self.clearance, &marking) {
            // Above clearance: never cached, never rendered.
            return Err(StoreError::AboveClearance);
        }
        let aad = aad_for(id, &marking);
        let sealed = self.kms.seal(&aad, plaintext, rng)?;
        self.objects
            .insert(id.to_string(), CachedObject { marking, sealed });
        Ok(())
    }

    /// Recover a cached object. Fails closed on a tampered blob, the wrong root, or
    /// — defence in depth — a marking the current clearance no longer dominates.
    pub fn get(&self, id: &str) -> Result<Vec<u8>, StoreError> {
        let obj = self.objects.get(id).ok_or(StoreError::NotFound)?;
        if !dominates(&self.clearance, &obj.marking) {
            return Err(StoreError::AboveClearance);
        }
        let aad = aad_for(id, &obj.marking);
        Ok(self.kms.unseal(&aad, &obj.sealed)?)
    }

    /// Whether an object is cached (regardless of current visibility).
    pub fn contains(&self, id: &str) -> bool {
        self.objects.contains_key(id)
    }

    /// The ids the current clearance may render — the safe set for a UI shell to
    /// list. `[AC-C-13]`
    pub fn visible_ids(&self) -> Vec<String> {
        self.objects
            .iter()
            .filter(|(_, o)| dominates(&self.clearance, &o.marking))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// The sealed blob for an object, for inspection/sync. Carries no plaintext.
    pub fn sealed(&self, id: &str) -> Option<&SealedBlob> {
        self.objects.get(id).map(|o| &o.sealed)
    }

    /// Number of cached objects.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// The clearance this store is scoped to.
    pub fn clearance(&self) -> &Clearance {
        &self.clearance
    }
}

/// Canonical additional-authenticated-data binding an object id and its marking
/// into the seal, so a sealed blob cannot be replayed under a different id or a
/// downgraded marking. Length-prefixed to be unambiguous.
fn aad_for(id: &str, marking: &Marking) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(id.len() as u32).to_le_bytes());
    v.extend_from_slice(id.as_bytes());
    v.push(marking.level.0);
    v.extend_from_slice(&(marking.compartments.len() as u32).to_le_bytes());
    for c in &marking.compartments {
        v.extend_from_slice(&(c.0.len() as u32).to_le_bytes());
        v.extend_from_slice(c.0.as_bytes());
    }
    v
}

// In-crate tests reach private fields to prove the at-rest seal fails closed on
// tamper and against a foreign keystore root — the AC-C-2 integrity guarantees.
#[cfg(all(test, feature = "backend-fips"))]
mod tests {
    use super::*;
    use midgard_client_keystore::SoftwareKeystore;
    use pqc_agility::backend::fips::provider as fips_provider;
    use std::collections::BTreeSet;

    struct SeedRng(u64);
    impl Rng for SeedRng {
        fn fill(&mut self, dest: &mut [u8]) {
            for b in dest {
                self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
                *b = (self.0 >> 33) as u8;
            }
        }
    }

    fn unclassified() -> Clearance {
        Clearance {
            level: policy::ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        }
    }
    fn marking_unclassified() -> Marking {
        Marking {
            level: policy::ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        }
    }

    fn store(seed: u64, clearance: Clearance) -> LocalStore<SoftwareKeystore> {
        let provider = fips_provider();
        let ks = SoftwareKeystore::generate(
            &provider,
            "device-root",
            SecurityFloor::HardwareBacked,
            &mut SeedRng(seed),
        )
        .unwrap();
        LocalStore::open(provider, ks, clearance, SecurityFloor::NonExtractable).unwrap()
    }

    #[test]
    fn a_tampered_blob_fails_closed() {
        let mut s = store(1, unclassified());
        s.put(
            "obj:1",
            marking_unclassified(),
            b"secret-bytes",
            &mut SeedRng(9),
        )
        .unwrap();

        // Corrupt one byte of the ciphertext and confirm unseal refuses it.
        let mut blob = s.objects.get("obj:1").unwrap().sealed.clone();
        blob.ciphertext[0] ^= 0xff;
        let aad = aad_for("obj:1", &marking_unclassified());
        assert!(
            s.kms.unseal(&aad, &blob).is_err(),
            "tamper must fail closed"
        );
    }

    #[test]
    fn a_foreign_root_cannot_unseal() {
        let mut a = store(1, unclassified());
        a.put(
            "obj:1",
            marking_unclassified(),
            b"secret-bytes",
            &mut SeedRng(9),
        )
        .unwrap();
        let blob = a.objects.get("obj:1").unwrap().sealed.clone();

        // A different device root cannot recover another device's cache.
        let b = store(2, unclassified());
        let aad = aad_for("obj:1", &marking_unclassified());
        assert!(
            b.kms.unseal(&aad, &blob).is_err(),
            "a foreign keystore root must fail closed"
        );
    }
}
