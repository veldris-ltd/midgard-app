//! Hybrid shared-secret combination. Per NIST SP 1800-38, a hybrid KEM must mix
//! the post-quantum and classical secrets through a KDF such that dropping either
//! input changes the output — this defeats the classic "hybrid that silently
//! ignores the PQ half" bug. [NFR-PQ-4] [NFR-PQ-1]

use crate::hash::hkdf_sha256_32;
use crate::kem::SharedSecret;

const HYBRID_SALT: &[u8] = b"midgard-hybrid-kem-v1";

/// Combine a post-quantum and a classical shared secret into one 32-byte secret,
/// bound to `context` (e.g. the negotiated suite id / transcript). Both inputs
/// feed the KDF, so a change to *either* changes the result.
pub fn combine_secrets(ss_pq: &[u8], ss_classical: &[u8], context: &[u8]) -> SharedSecret {
    let mut ikm = Vec::with_capacity(ss_pq.len() + ss_classical.len());
    ikm.extend_from_slice(ss_pq);
    ikm.extend_from_slice(ss_classical);
    let out = hkdf_sha256_32(HYBRID_SALT, &ikm, context);
    SharedSecret::from_bytes(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_halves_mix() {
        let base = *combine_secrets(&[1; 32], &[2; 32], b"ctx").expose();
        let changed_pq = *combine_secrets(&[9; 32], &[2; 32], b"ctx").expose();
        let changed_cl = *combine_secrets(&[1; 32], &[9; 32], b"ctx").expose();
        let changed_ctx = *combine_secrets(&[1; 32], &[2; 32], b"other").expose();
        assert_ne!(
            base, changed_pq,
            "changing the PQ half must change the secret"
        );
        assert_ne!(
            base, changed_cl,
            "changing the classical half must change the secret"
        );
        assert_ne!(base, changed_ctx, "context must bind the secret");
    }
}
