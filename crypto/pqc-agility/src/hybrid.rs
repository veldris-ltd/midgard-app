//! Hybrid shared-secret combination: the **X-Wing** combiner
//! (draft-connolly-cfrg-xwing-kem), which NIST SP 800-227 §4.6 recognises as an
//! approved way to combine an ML-KEM secret with a classical one. [NFR-PQ-4]
//! [NFR-PQ-1] [MG-SEC-09]
//!
//! ```text
//! ss = SHA3-256( ss_M ‖ ss_X ‖ ct_X ‖ pk_X ‖ label )
//! ```
//!
//! Unlike the home-grown HKDF mix it replaces, the combiner binds the classical
//! key-exchange messages (`ct_X`, the ephemeral X25519 public key, and `pk_X`, the
//! recipient's X25519 public key), so the X25519 half cannot be swapped for another
//! exchange that happens to produce the same DH output. ML-KEM's own secret already
//! binds its ciphertext (FIPS 203's Fujisaki–Okamoto transform), which is why X-Wing
//! does not hash `ct_M`.
//!
//! The label is X-Wing's six-byte `\.//^\` for the ML-KEM-768 suite, so that suite
//! is X-Wing exactly. ML-KEM-1024 + X25519 is not covered by the X-Wing draft; it uses
//! the same construction under its own label, so the two suites never share a key.
//!
//! The degenerate all-zero X25519 output (a low-order or identity public key) is
//! refused by the backends **before** this combiner runs — see
//! [`reject_noncontributory_x25519`].

use sha3::{Digest as _, Sha3_256};
use zeroize::Zeroizing;

use crate::algorithm::KemSuiteId;
use crate::error::CryptoError;
use crate::kem::SharedSecret;

/// X-Wing's combiner label, `\.//^\`.
pub const XWING_LABEL: &[u8; 6] = b"\\.//^\\";

/// The combiner label for the ML-KEM-1024 + X25519 suite (outside the X-Wing draft).
pub const XWING_1024_LABEL: &[u8] = b"midgard-xwing-mlkem1024-x25519-v1";

/// The label the combiner uses for `suite`.
pub fn combiner_label(suite: KemSuiteId) -> &'static [u8] {
    match suite {
        KemSuiteId::HybridX25519MlKem1024 => XWING_1024_LABEL,
        _ => XWING_LABEL,
    }
}

/// Combine the ML-KEM secret `ss_m` and the X25519 secret `ss_x` with the X25519
/// ciphertext (the ephemeral public key) `ct_x` and the recipient's X25519 public key
/// `pk_x`, under `suite`'s label. A change to any input changes the result.
pub fn combine_hybrid(
    suite: KemSuiteId,
    ss_m: &[u8; 32],
    ss_x: &[u8; 32],
    ct_x: &[u8; 32],
    pk_x: &[u8; 32],
) -> SharedSecret {
    let mut h = Sha3_256::new();
    h.update(ss_m);
    h.update(ss_x);
    h.update(ct_x);
    h.update(pk_x);
    h.update(combiner_label(suite));
    let out = Zeroizing::new(<[u8; 32]>::from(h.finalize()));
    SharedSecret::from_bytes(*out)
}

/// Refuse an all-zero X25519 output. RFC 7748 §6.1: a peer that sends a low-order
/// point forces the DH result to zero, so that half of the hybrid would contribute no
/// secret at all. Constant-time over the 32 bytes. [MG-SEC-09]
pub fn reject_noncontributory_x25519(ss_x: &[u8; 32]) -> Result<(), CryptoError> {
    if crate::hash::ct_eq(ss_x, &[0u8; 32]) {
        Err(CryptoError::InvalidKey)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: KemSuiteId = KemSuiteId::HybridX25519MlKem768;

    #[test]
    fn every_input_binds_the_secret() {
        let base = *combine_hybrid(S, &[1; 32], &[2; 32], &[3; 32], &[4; 32]).expose();
        for (what, other) in [
            (
                "ss_M",
                combine_hybrid(S, &[9; 32], &[2; 32], &[3; 32], &[4; 32]),
            ),
            (
                "ss_X",
                combine_hybrid(S, &[1; 32], &[9; 32], &[3; 32], &[4; 32]),
            ),
            (
                "ct_X",
                combine_hybrid(S, &[1; 32], &[2; 32], &[9; 32], &[4; 32]),
            ),
            (
                "pk_X",
                combine_hybrid(S, &[1; 32], &[2; 32], &[3; 32], &[9; 32]),
            ),
            (
                "suite",
                combine_hybrid(
                    KemSuiteId::HybridX25519MlKem1024,
                    &[1; 32],
                    &[2; 32],
                    &[3; 32],
                    &[4; 32],
                ),
            ),
        ] {
            assert_ne!(
                &base,
                other.expose(),
                "changing {what} must change the secret"
            );
        }
    }

    #[test]
    fn matches_the_xwing_formula() {
        // ss = SHA3-256(ss_M ‖ ss_X ‖ ct_X ‖ pk_X ‖ "\.//^\") computed independently.
        let mut buf = Vec::new();
        for b in [1u8, 2, 3, 4] {
            buf.extend_from_slice(&[b; 32]);
        }
        buf.extend_from_slice(b"\\.//^\\");
        let want: [u8; 32] = Sha3_256::digest(&buf).into();
        let got = combine_hybrid(S, &[1; 32], &[2; 32], &[3; 32], &[4; 32]);
        assert_eq!(got.expose(), &want);
        assert_eq!(XWING_LABEL, &[0x5c, 0x2e, 0x2f, 0x2f, 0x5e, 0x5c]);
    }

    #[test]
    fn an_all_zero_x25519_result_is_refused() {
        assert!(reject_noncontributory_x25519(&[0; 32]).is_err());
        let mut one = [0u8; 32];
        one[31] = 1;
        assert!(reject_noncontributory_x25519(&one).is_ok());
    }
}
