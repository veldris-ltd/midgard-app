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
//! 3. **Persisted whole and sealed.** [`export_sealed`](LocalStore::export_sealed)
//!    writes the cache as one blob sealed again under the same root, so ids,
//!    markings and the scope (the signed-in persona) are not readable at rest
//!    either; [`import_sealed`](LocalStore::import_sealed) loads it all or nothing.
//!    A store is bound to one scope: a file written for another scope does not
//!    open, and [`rescope`](LocalStore::rescope) to another scope drops every row.
//!    `[AC-C-2]` `[AC-C-13]`

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
    /// Whose cache this is (a persona id); bound into the exported file's seal.
    scope: String,
    objects: BTreeMap<String, CachedObject>,
}

/// File magic and format version of an exported cache (inside the outer seal).
pub const CACHE_MAGIC: &[u8; 8] = b"MGCACHE1";
/// AAD binding the outer seal to its purpose; the scope is appended.
const CACHE_FILE_AAD: &[u8] = b"midgard-client/cache-file/v1";

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
        Self::open_scoped(provider, keystore, clearance, min_floor, "")
    }

    /// [`open`](Self::open) for one scope (the signed-in persona): its exported file
    /// opens only for the same scope. `[AC-C-13]`
    pub fn open_scoped(
        provider: CryptoProvider,
        keystore: R,
        clearance: Clearance,
        min_floor: SecurityFloor,
        scope: &str,
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
            scope: scope.to_string(),
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

    /// The scope (persona) this store is bound to.
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// The whole cache as one sealed blob: entries (id, marking, sealed object)
    /// inside an outer seal under the same root, bound to this store's scope, so
    /// neither ids, markings nor the scope are readable at rest. `[AC-C-2]`
    pub fn export_sealed(&self, rng: &mut dyn Rng) -> Result<Vec<u8>, StoreError> {
        let mut inner = CACHE_MAGIC.to_vec();
        inner.extend_from_slice(&(self.objects.len() as u32).to_le_bytes());
        for (id, obj) in &self.objects {
            put(&mut inner, id.as_bytes());
            inner.push(obj.marking.level.0);
            inner.extend_from_slice(&(obj.marking.compartments.len() as u32).to_le_bytes());
            for c in &obj.marking.compartments {
                put(&mut inner, c.0.as_bytes());
            }
            put(&mut inner, &obj.sealed.encode());
        }
        let file = self.kms.seal(&self.file_aad(), &inner, rng)?.encode();
        Ok(file)
    }

    /// Load an exported cache, replacing what is cached. All or nothing: a file that
    /// does not open (another root or scope, any changed or missing byte) or does
    /// not parse (structure, format version) is refused and the store is left as it
    /// was — the caller discards the file and refetches. Rows the current clearance
    /// does not dominate are dropped, never stored. Returns `(kept, dropped)`.
    /// `[AC-C-13]` `[AC-C-2]`
    pub fn import_sealed(&mut self, file: &[u8]) -> Result<(usize, usize), StoreError> {
        let outer = SealedBlob::decode(file)?;
        let inner = self.kms.unseal(&self.file_aad(), &outer)?;
        let bad = |d: &str| StoreError::Malformed(d.to_string());
        let mut c = Cursor { b: &inner, at: 0 };
        let magic = c.take(CACHE_MAGIC.len()).ok_or_else(|| bad("truncated"))?;
        if magic[..7] != CACHE_MAGIC[..7] {
            return Err(bad("not a cache file"));
        }
        if magic != CACHE_MAGIC {
            return Err(bad("unsupported cache format version"));
        }
        let count = c.u32().ok_or_else(|| bad("count"))? as usize;
        let mut objects = BTreeMap::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut dropped = 0;
        for _ in 0..count {
            let id = String::from_utf8(c.field().ok_or_else(|| bad("id"))?.to_vec())
                .map_err(|_| bad("id"))?;
            let level = policy::ClassificationLevel(c.take(1).ok_or_else(|| bad("level"))?[0]);
            let nc = c.u32().ok_or_else(|| bad("compartments"))? as usize;
            let mut compartments = std::collections::BTreeSet::new();
            for _ in 0..nc {
                let name = c.field().ok_or_else(|| bad("compartment"))?;
                let name = String::from_utf8(name.to_vec()).map_err(|_| bad("compartment"))?;
                compartments.insert(policy::Compartment(name));
            }
            let sealed = SealedBlob::decode(c.field().ok_or_else(|| bad("object"))?)?;
            if sealed.root_key_id != self.kms.root_key_id() {
                return Err(StoreError::Kms(kms::KmsError::WrongTrustRoot));
            }
            let marking = Marking {
                level,
                compartments,
            };
            if !seen.insert(id.clone()) {
                return Err(bad("an id twice"));
            }
            if dominates(&self.clearance, &marking) {
                objects.insert(id, CachedObject { marking, sealed });
            } else {
                dropped += 1;
            }
        }
        if c.at != inner.len() {
            return Err(bad("trailing bytes"));
        }
        let kept = objects.len();
        self.objects = objects;
        Ok((kept, dropped))
    }

    /// Re-scope the store (a persona switch). To another scope, every row is
    /// dropped, whatever the new clearance: one persona never reads another's
    /// cache. To the same scope, rows `clearance` does not dominate are dropped.
    /// Returns the store and how many rows were dropped. `[AC-C-13]`
    pub fn rescope(mut self, scope: &str, clearance: Clearance) -> (Self, usize) {
        let before = self.objects.len();
        if scope != self.scope {
            self.objects.clear();
            self.scope = scope.to_string();
        } else {
            self.objects
                .retain(|_, o| dominates(&clearance, &o.marking));
        }
        self.clearance = clearance;
        let dropped = before - self.objects.len();
        (self, dropped)
    }

    /// Remove one cached object.
    pub fn remove(&mut self, id: &str) {
        self.objects.remove(id);
    }

    /// Remove every cached object ("Reset demo", or a cache that failed to load).
    pub fn clear(&mut self) {
        self.objects.clear();
    }

    fn file_aad(&self) -> Vec<u8> {
        let mut aad = CACHE_FILE_AAD.to_vec();
        put(&mut aad, self.scope.as_bytes());
        aad
    }
}

fn put(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

/// A bounds-checked reader: `None` on any short read, never a panic.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len())?;
        let s = &self.b[self.at..end];
        self.at = end;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn field(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
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
        let ks = SoftwareKeystore::generate(&provider, "device-root", &mut SeedRng(seed)).unwrap();
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

    /// A file that opens under this root and scope, holding `inner`.
    fn file_holding(s: &LocalStore<SoftwareKeystore>, inner: &[u8]) -> Vec<u8> {
        s.kms
            .seal(&s.file_aad(), inner, &mut SeedRng(5))
            .unwrap()
            .encode()
    }

    #[test]
    fn another_cache_format_version_is_refused_and_nothing_is_loaded() {
        let mut s = store(1, unclassified());
        s.put("kept", marking_unclassified(), b"x", &mut SeedRng(9))
            .unwrap();
        let mut inner = b"MGCACHE2".to_vec();
        inner.extend_from_slice(&0u32.to_le_bytes());
        let file = file_holding(&s, &inner);
        assert!(matches!(
            s.import_sealed(&file),
            Err(StoreError::Malformed(d)) if d.contains("version")
        ));
        assert!(s.contains("kept"));
    }

    #[test]
    fn a_cache_file_with_a_bad_inside_is_refused_and_nothing_is_loaded() {
        let mut s = store(1, unclassified());
        s.put("a", marking_unclassified(), b"x", &mut SeedRng(9))
            .unwrap();
        let good = s.export_sealed(&mut SeedRng(3)).unwrap();
        let inner = s
            .kms
            .unseal(&s.file_aad(), &SealedBlob::decode(&good).unwrap())
            .unwrap();
        // The one entry twice (count 2), trailing bytes, a count larger than the
        // entries, and an entry cut short.
        let entry = &inner[12..];
        let mut twice = CACHE_MAGIC.to_vec();
        twice.extend_from_slice(&2u32.to_le_bytes());
        twice.extend_from_slice(entry);
        twice.extend_from_slice(entry);
        let mut trailing = inner.clone();
        trailing.push(0);
        let mut more = inner.clone();
        more[8] = 3;
        let short = inner[..inner.len() - 1].to_vec();
        for (what, bad) in [
            ("twice", twice),
            ("trailing", trailing),
            ("count", more),
            ("short", short),
        ] {
            s.clear();
            s.put("b", marking_unclassified(), b"y", &mut SeedRng(9))
                .unwrap();
            let file = file_holding(&s, &bad);
            assert!(
                matches!(s.import_sealed(&file), Err(StoreError::Malformed(_))),
                "{what}"
            );
            assert!(s.contains("b") && s.len() == 1, "{what}: store changed");
        }
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
