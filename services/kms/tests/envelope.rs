//! AC-M-2: data at rest is AES-256-GCM under an ML-KEM-1024-wrapped key sealed to
//! the trust root; unsealing requires the trust root and fails closed otherwise.
//! [NFR-PQ-2]

use kms::{Kms, SealedBlob, SoftwareTrustRoot, WRAP_SUITE};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{KemSuiteId, Rng};

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

fn kms_with_root(seed: u64, key_id: &str) -> Kms<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, key_id, &mut SeedRng(seed)).unwrap();
    Kms::new(provider, root)
}

#[test]
fn seal_is_ml_kem_1024_and_carries_no_plaintext() {
    let kms = kms_with_root(1, "root-1");
    let secret = b"OBJECT STORE ROW: salary=125000; ssn=000-00-0000";
    let blob = kms.seal(b"obj:42", secret, &mut SeedRng(10)).unwrap();

    // The data key is wrapped by ML-KEM-1024 and sealed to the named root.
    assert_eq!(blob.suite, WRAP_SUITE);
    assert_eq!(blob.suite, KemSuiteId::MlKem1024);
    assert_eq!(blob.root_key_id, "root-1");
    // The sealed bytes do not contain the plaintext.
    assert!(!contains(&blob.ciphertext, secret));
    // AES-256-GCM adds a 16-byte tag.
    assert_eq!(blob.ciphertext.len(), secret.len() + 16);
}

#[test]
fn roundtrip_through_the_trust_root() {
    let kms = kms_with_root(2, "root-2");
    let secret = b"at-rest payload";
    let blob = kms.seal(b"aad", secret, &mut SeedRng(11)).unwrap();
    let recovered = kms.unseal(b"aad", &blob).unwrap();
    assert_eq!(&recovered, secret);
}

#[test]
fn a_different_trust_root_cannot_unseal() {
    let kms_a = kms_with_root(3, "root-a");
    let blob = kms_a.seal(b"aad", b"top secret", &mut SeedRng(12)).unwrap();

    // A KMS with a different root of the SAME id cannot decapsulate the wrapped
    // key, so the AEAD open fails (data is bound to the actual trust root).
    let provider = fips_provider();
    let other_root = SoftwareTrustRoot::generate(&provider, "root-a", &mut SeedRng(999)).unwrap();
    let kms_b = Kms::new(provider, other_root);
    assert!(kms_b.unseal(b"aad", &blob).is_err());
}

#[test]
fn wrong_root_id_is_rejected() {
    let kms_a = kms_with_root(4, "root-a");
    let mut blob = kms_a.seal(b"aad", b"data", &mut SeedRng(13)).unwrap();
    blob.root_key_id = "root-z".into();
    assert!(kms_a.unseal(b"aad", &blob).is_err());
}

#[test]
fn tampered_ciphertext_or_wrapped_key_fails_closed() {
    let kms = kms_with_root(5, "root-5");
    let blob = kms
        .seal(b"aad", b"important data", &mut SeedRng(14))
        .unwrap();

    // Flip a ciphertext byte → AEAD tag fails.
    let mut t1 = blob.clone();
    t1.ciphertext[0] ^= 0x01;
    assert!(kms.unseal(b"aad", &t1).is_err());

    // Flip a wrapped-key byte → different shared secret → wrong DEK → AEAD fails.
    let mut t2 = blob.clone();
    let mut wk = t2.wrapped_key.as_bytes().to_vec();
    wk[0] ^= 0x01;
    t2.wrapped_key = pqc_agility::Ciphertext::new(t2.suite, wk);
    assert!(kms.unseal(b"aad", &t2).is_err());

    // Wrong AAD → AEAD fails.
    assert!(kms.unseal(b"different-aad", &blob).is_err());
}

#[test]
fn distinct_seals_use_distinct_wrapped_keys() {
    // Each seal encapsulates freshly, so two seals of the same plaintext differ.
    let kms = kms_with_root(6, "root-6");
    let a: SealedBlob = kms.seal(b"aad", b"same", &mut SeedRng(1)).unwrap();
    let b: SealedBlob = kms.seal(b"aad", b"same", &mut SeedRng(2)).unwrap();
    assert_ne!(a.wrapped_key, b.wrapped_key);
    assert_ne!(a.ciphertext, b.ciphertext);
    // Both still unseal to the same plaintext.
    assert_eq!(kms.unseal(b"aad", &a).unwrap(), b"same");
    assert_eq!(kms.unseal(b"aad", &b).unwrap(), b"same");
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
