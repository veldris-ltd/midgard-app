//! A sealed root reopens only through the platform key root that sealed its data
//! key, is the same ML-KEM root afterwards, reports the custody it has, and fails
//! closed on any change to the blob. [FR-C-2] [NFR-PQ-2] [VLD-ANB-MIDGARD-0056]
#![cfg(feature = "backend-fips")]

use std::cell::Cell;

use kms::{Kms, TrustRoot};
use midgard_client_keystore::sealed_root::{RootSealer, SealedRootKeystore};
use midgard_client_keystore::{Keystore, KeystoreError, SecurityFloor};
use pqc_agility::backend::fips::provider;
use pqc_agility::{Aes256GcmAead, OsRng, Rng, SecretBytes, GCM_NONCE_LEN};

/// A software stand-in for the Secure Enclave: AES-256-GCM under a device secret.
/// The real one seals with the Enclave key (ECIES) and prompts on unseal.
struct TestSealer {
    device: [u8; 32],
    hardware: bool,
    unseals: Cell<usize>,
}

fn sealer(byte: u8) -> TestSealer {
    TestSealer {
        device: [byte; 32],
        hardware: false,
        unseals: Cell::new(0),
    }
}

impl RootSealer for TestSealer {
    fn seal(&self, data_key: &SecretBytes) -> Result<Vec<u8>, KeystoreError> {
        let mut nonce = [0u8; GCM_NONCE_LEN];
        OsRng.fill(&mut nonce);
        let mut out = nonce.to_vec();
        out.extend(
            Aes256GcmAead::new(&self.device)
                .seal(&nonce, b"test-sealer", data_key.expose())
                .unwrap(),
        );
        Ok(out)
    }
    fn unseal(&self, sealed: &[u8]) -> Result<SecretBytes, KeystoreError> {
        self.unseals.set(self.unseals.get() + 1);
        let nonce: [u8; GCM_NONCE_LEN] = sealed
            .get(..GCM_NONCE_LEN)
            .and_then(|n| n.try_into().ok())
            .ok_or(KeystoreError::Unseal)?;
        Aes256GcmAead::new(&self.device)
            .open(&nonce, b"test-sealer", &sealed[GCM_NONCE_LEN..])
            .map(SecretBytes::new)
            .map_err(|_| KeystoreError::Unseal)
    }
    fn hardware_backed(&self) -> bool {
        self.hardware
    }
}

/// A broken platform side that "seals" by returning the key itself.
struct IdentitySealer;
impl RootSealer for IdentitySealer {
    fn seal(&self, data_key: &SecretBytes) -> Result<Vec<u8>, KeystoreError> {
        let mut v = b"prefix".to_vec();
        v.extend_from_slice(data_key.expose());
        Ok(v)
    }
    fn unseal(&self, sealed: &[u8]) -> Result<SecretBytes, KeystoreError> {
        Ok(SecretBytes::new(sealed[6..].to_vec()))
    }
    fn hardware_backed(&self) -> bool {
        true
    }
}

/// A platform side whose user cancels the prompt.
struct Cancelled;
impl RootSealer for Cancelled {
    fn seal(&self, _: &SecretBytes) -> Result<Vec<u8>, KeystoreError> {
        Err(KeystoreError::UserCancelled)
    }
    fn unseal(&self, _: &[u8]) -> Result<SecretBytes, KeystoreError> {
        Err(KeystoreError::UserCancelled)
    }
    fn hardware_backed(&self) -> bool {
        true
    }
}

fn create(s: &dyn RootSealer) -> (SealedRootKeystore, Vec<u8>) {
    SealedRootKeystore::create(&provider(), "device-root", s, &mut OsRng).unwrap()
}

#[test]
fn a_sealed_root_reopens_as_the_same_root() {
    let s = sealer(7);
    let (ks, blob) = create(&s);
    let sealed = Kms::new(provider(), ks)
        .seal(b"aad", b"payload", &mut OsRng)
        .unwrap();
    let again = SealedRootKeystore::open(&blob, &s).unwrap();
    assert_eq!(s.unseals.get(), 1, "one unseal (one prompt) per open");
    assert_eq!(Keystore::key_id(&again), "device-root");
    assert_eq!(
        Kms::new(provider(), again).unseal(b"aad", &sealed).unwrap(),
        b"payload"
    );
}

#[test]
fn the_floor_is_non_extractable_even_when_the_platform_key_is_hardware() {
    let mut s = sealer(7);
    s.hardware = true;
    let (ks, blob) = create(&s);
    assert_eq!(ks.floor(), SecurityFloor::NonExtractable);
    assert!(ks.hardware_wrapped());
    assert!(ks.holds_key());
    let again = SealedRootKeystore::open(&blob, &s).unwrap();
    assert_eq!(again.floor(), SecurityFloor::NonExtractable);
    assert!(again.hardware_wrapped());
    assert!(!create(&sealer(7)).0.hardware_wrapped());
}

#[test]
fn another_platform_key_cannot_reopen_it() {
    let (_, blob) = create(&sealer(7));
    assert!(matches!(
        SealedRootKeystore::open(&blob, &sealer(8)),
        Err(KeystoreError::Unseal)
    ));
}

#[test]
fn any_flipped_byte_or_truncation_fails_closed() {
    let s = sealer(7);
    let (_, blob) = create(&s);
    let _: &dyn TrustRoot = &SealedRootKeystore::open(&blob, &s).unwrap();
    // Every byte of the header, the wrapped key, the nonce and the tag; a stride
    // through the long fields (public key, encrypted secret).
    let mut at: Vec<usize> = (0..64).collect();
    at.extend((64..blob.len()).step_by(97));
    at.extend(blob.len() - 32..blob.len());
    for i in at {
        let mut bad = blob.clone();
        bad[i] ^= 0x01;
        assert!(
            SealedRootKeystore::open(&bad, &s).is_err(),
            "byte {i} flipped was accepted"
        );
    }
    for n in [0, 7, 8, 12, blob.len() / 2, blob.len() - 1] {
        assert!(
            SealedRootKeystore::open(&blob[..n], &s).is_err(),
            "truncated to {n} was accepted"
        );
    }
    let mut longer = blob.clone();
    longer.push(0);
    assert!(matches!(
        SealedRootKeystore::open(&longer, &s),
        Err(KeystoreError::Malformed(_))
    ));
}

#[test]
fn another_format_version_is_refused_as_malformed() {
    let s = sealer(7);
    let (_, mut blob) = create(&s);
    blob[7] = b'2'; // "MGSROOT2"
    assert!(matches!(
        SealedRootKeystore::open(&blob, &s),
        Err(KeystoreError::Malformed(_))
    ));
}

#[test]
fn a_platform_side_that_does_not_seal_is_refused() {
    assert!(matches!(
        SealedRootKeystore::create(&provider(), "device-root", &IdentitySealer, &mut OsRng),
        Err(KeystoreError::PlatformUnavailable(_))
    ));
}

/// A platform side whose output is too short to be a sealed key (it would need at
/// least the key and a 16-byte tag).
struct TooShort;
impl RootSealer for TooShort {
    fn seal(&self, _: &SecretBytes) -> Result<Vec<u8>, KeystoreError> {
        let mut v = vec![0u8; 47];
        OsRng.fill(&mut v);
        Ok(v)
    }
    fn unseal(&self, _: &[u8]) -> Result<SecretBytes, KeystoreError> {
        Err(KeystoreError::Unseal)
    }
    fn hardware_backed(&self) -> bool {
        true
    }
}

#[test]
fn a_sealed_key_shorter_than_key_and_tag_is_refused() {
    assert!(matches!(
        SealedRootKeystore::create(&provider(), "device-root", &TooShort, &mut OsRng),
        Err(KeystoreError::PlatformUnavailable(_))
    ));
}

#[test]
fn a_cancelled_prompt_is_reported_as_such() {
    assert!(matches!(
        SealedRootKeystore::create(&provider(), "device-root", &Cancelled, &mut OsRng),
        Err(KeystoreError::UserCancelled)
    ));
    let (_, blob) = create(&sealer(7));
    assert!(matches!(
        SealedRootKeystore::open(&blob, &Cancelled),
        Err(KeystoreError::UserCancelled)
    ));
}
