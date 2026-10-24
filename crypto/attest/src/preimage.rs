//! The release bundle's signed body.
//!
//! Everything the signer emits is bound into the bytes that get signed, so there
//! is no way to swap content under a valid signature — the same discipline as
//! `bifrost_artefact::ArtefactManifest::encode`, applied to the on-disk release
//! bundle. `[NFR-PQ-3]` `[AC-B-1]` `[AC-B-4]`
//!
//! ```text
//! body     = artefact_sha256 ‖ sbom_sha256 ‖ attestation_sha256
//!               (32 bytes)      (32 bytes)        (32 bytes)
//! preimage = accept::preimage(RELEASE_ARTEFACT, body)
//! ```
//!
//! Every field is fixed-width, so the body is unambiguous without length
//! prefixes; the domain prefix is applied by [`crate::accept::preimage`], which
//! is what keeps this construct's signatures from being interchangeable with
//! Bifrost's or Mimir's.
//!
//! ## Why the attestation digest is in here
//! The RF-SC-1 contract specifies `digest ‖ sha256(sbom_bytes)`. This
//! implementation additionally binds the attestation. Without it the `.att` is
//! unsigned metadata that an attacker can swap for one from another build, and
//! catching that would depend on parsing attacker-controlled JSON correctly —
//! trading a cryptographic guarantee for a parser. With it, every tamper across
//! all three files is one signature failure, and `verify` needs no JSON parser at
//! all. `docs/dev/markdown/SAD_09_Completion_Plan.md` (RF-SC-1) records this deviation.

use pqc_agility::hash::sha256;

use crate::accept::{self, RELEASE_ARTEFACT};

/// The domain-separation label for release-artefact signatures, as written into
/// the `.sig` envelope.
pub const DOMAIN_LABEL: &[u8] = RELEASE_ARTEFACT.label();

/// Length of the signed body: three SHA-256 digests.
pub const BODY_LEN: usize = 32 * 3;

/// The three digests this construct binds, in canonical order.
pub fn body(
    artefact_sha256: &[u8; 32],
    sbom_sha256: &[u8; 32],
    attestation_sha256: &[u8; 32],
) -> Vec<u8> {
    let mut o = Vec::with_capacity(BODY_LEN);
    o.extend_from_slice(artefact_sha256);
    o.extend_from_slice(sbom_sha256);
    o.extend_from_slice(attestation_sha256);
    o
}

/// The full canonical preimage, domain included.
pub fn preimage(
    artefact_sha256: &[u8; 32],
    sbom_sha256: &[u8; 32],
    attestation_sha256: &[u8; 32],
) -> Vec<u8> {
    accept::preimage(
        &RELEASE_ARTEFACT,
        &body(artefact_sha256, sbom_sha256, attestation_sha256),
    )
}

/// SHA-256 as a fixed array, for callers that hold `&[u8]`.
pub fn digest(bytes: &[u8]) -> [u8; 32] {
    sha256(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: [u8; 32] = [1u8; 32];
    const B: [u8; 32] = [2u8; 32];
    const C: [u8; 32] = [3u8; 32];

    #[test]
    fn body_is_fixed_width() {
        assert_eq!(body(&A, &B, &C).len(), BODY_LEN);
        assert_eq!(&body(&A, &B, &C)[..32], &A);
    }

    #[test]
    fn preimage_carries_the_release_domain() {
        let p = preimage(&A, &B, &C);
        assert_eq!(p[0] as usize, DOMAIN_LABEL.len());
        assert_eq!(&p[1..1 + DOMAIN_LABEL.len()], DOMAIN_LABEL);
        assert_eq!(p.len(), 1 + DOMAIN_LABEL.len() + BODY_LEN);
    }

    #[test]
    fn every_field_changes_the_preimage() {
        let base = preimage(&A, &B, &C);
        assert_ne!(preimage(&C, &B, &C), base, "artefact digest is bound");
        assert_ne!(preimage(&A, &C, &C), base, "sbom digest is bound");
        assert_ne!(preimage(&A, &B, &A), base, "attestation digest is bound");
    }

    #[test]
    fn fields_are_not_interchangeable() {
        // A permutation of the same digests must not produce the same preimage —
        // otherwise an SBOM digest could be presented as an artefact digest.
        assert_ne!(preimage(&A, &B, &C), preimage(&B, &A, &C));
        assert_ne!(preimage(&A, &B, &C), preimage(&A, &C, &B));
    }

    #[test]
    fn is_deterministic() {
        assert_eq!(preimage(&A, &B, &C), preimage(&A, &B, &C));
    }

    #[test]
    fn cannot_collide_with_another_signing_context() {
        use crate::accept::{BIFROST_ARTEFACT, MIMIR_MODEL};
        let body = body(&A, &B, &C);
        assert_ne!(
            preimage(&A, &B, &C),
            accept::preimage(&BIFROST_ARTEFACT, &body)
        );
        assert_ne!(preimage(&A, &B, &C), accept::preimage(&MIMIR_MODEL, &body));
    }
}
