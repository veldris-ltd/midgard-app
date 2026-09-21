//! RC-28 — Given a hardware root, when data is sealed/unsealed, then the root secret
//! never enters process memory (decapsulation is delegated to the device), and
//! rotation re-wraps new data without touching old blobs. Root attestation is
//! verified and fails closed on tampering. [NFR-PQ-2] [RC-28]
#![cfg(feature = "backend-fips")]

use kms::hardware::HardwareToken;
use kms::{HardwareTrustRoot, Kms, RotatingKms, SoftHsmToken, SoftwareTrustRoot, WRAP_SUITE};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

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

const AAD: &[u8] = b"rc-28";

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn hardware_root_seals_and_unseals_over_the_device_boundary() {
    // Provision a (simulated) device and adopt it as the hardware trust root. The
    // KMS holds only the token handle — no secret-key bytes in this struct.
    let token = SoftHsmToken::provision("tpm-sim-0", "root-hw-1", &mut SeedRng(1)).unwrap();
    let hw_root = HardwareTrustRoot::new(Box::new(token));
    let kms = Kms::new(fips_provider(), hw_root);

    let blob = kms
        .seal(AAD, b"classified-payload", &mut SeedRng(2))
        .unwrap();
    assert_eq!(blob.root_key_id, "root-hw-1");
    assert!(
        !contains(&blob.encode(), b"classified-payload"),
        "no plaintext at rest"
    );

    let opened = kms.unseal(AAD, &blob).unwrap();
    assert_eq!(opened, b"classified-payload");
}

#[test]
fn decapsulation_happens_on_the_device_not_in_the_process() {
    // The process encapsulates using only the PUBLIC key (no secret needed), then
    // asks the device to decapsulate. The device counter proves the unwrap ran on
    // the token; the process never held the secret key.
    let token = SoftHsmToken::provision("tpm-sim-0", "root-hw-1", &mut SeedRng(1)).unwrap();
    assert_eq!(token.decapsulation_count(), 0);

    let provider = fips_provider();
    let kem = provider.kem_for(WRAP_SUITE).unwrap();
    // Encapsulate to the device's public key (a public-only operation).
    let (wrapped, ss_sender) = kem
        .encapsulate(token.public_key(), &mut SeedRng(2))
        .unwrap();

    // Decapsulate ON the device → identical shared secret, counter increments.
    let ss_device = token.decapsulate(&wrapped).unwrap();
    assert_eq!(ss_sender.expose(), ss_device.expose());
    assert_eq!(token.decapsulation_count(), 1);
    let _ = token.decapsulate(&wrapped).unwrap();
    assert_eq!(token.decapsulation_count(), 2);
}

#[test]
fn root_attestation_verifies_and_fails_closed_on_tamper() {
    let provider = fips_provider();
    let token = SoftHsmToken::provision("hsm-A", "root-hw-1", &mut SeedRng(7)).unwrap();

    // A genuine quote verifies under the device attestation key.
    let good = token.attest();
    good.verify(&provider, token.attestation_key())
        .expect("genuine attestation verifies");

    // Tamper: clear the non-exportable claim → rejected.
    let mut forged = token.attest();
    forged.non_exportable = false;
    assert!(
        forged.verify(&provider, token.attestation_key()).is_err(),
        "an exportable-key claim must be rejected"
    );

    // A different device's attestation key must not verify this quote.
    let other = SoftHsmToken::provision("hsm-B", "root-hw-2", &mut SeedRng(8)).unwrap();
    assert!(
        good.verify(&provider, other.attestation_key()).is_err(),
        "wrong attestation key must fail"
    );

    // The root-level check also binds the quote to this root's id + public key.
    let hw_root = HardwareTrustRoot::new(Box::new(token));
    hw_root
        .verify_attestation(&provider)
        .expect("root attestation binds id + public key");
}

#[test]
fn rotation_rewraps_new_data_without_touching_old_blobs() {
    // Start on a software root (dev), then rotate to a hardware root.
    let provider_a = fips_provider();
    let root_a = SoftwareTrustRoot::generate(&provider_a, "root-A", &mut SeedRng(10)).unwrap();
    let mut ring = RotatingKms::new(fips_provider(), Box::new(root_a));
    assert_eq!(ring.current_key_id(), "root-A");

    // Seal blob1 under root-A; remember its exact bytes.
    let blob1 = ring.seal(AAD, b"data-under-A", &mut SeedRng(11)).unwrap();
    assert_eq!(blob1.root_key_id, "root-A");
    let blob1_before = blob1.encode();

    // Rotate to a hardware root (root-B).
    let token_b = SoftHsmToken::provision("hsm-B", "root-B", &mut SeedRng(12)).unwrap();
    ring.rotate_to(Box::new(HardwareTrustRoot::new(Box::new(token_b))))
        .unwrap();
    assert_eq!(ring.current_key_id(), "root-B");

    // New data seals under root-B; blob1's bytes are untouched by the rotation.
    let blob2 = ring.seal(AAD, b"data-under-B", &mut SeedRng(13)).unwrap();
    assert_eq!(blob2.root_key_id, "root-B");
    assert_eq!(blob1.encode(), blob1_before, "old blob was not rewritten");

    // The keyring unseals BOTH, dispatching each blob to its own root.
    assert_eq!(ring.unseal(AAD, &blob1).unwrap(), b"data-under-A");
    assert_eq!(ring.unseal(AAD, &blob2).unwrap(), b"data-under-B");
    assert_eq!(ring.known_root_ids(), vec!["root-A", "root-B"]);

    // Retiring root-A drops its blobs (fail closed); root-B is unaffected.
    ring.retire("root-A").unwrap();
    assert!(matches!(
        ring.unseal(AAD, &blob1),
        Err(kms::KmsError::UnknownRoot(_))
    ));
    assert_eq!(ring.unseal(AAD, &blob2).unwrap(), b"data-under-B");
    // The current root cannot be retired.
    assert!(ring.retire("root-B").is_err());
}
