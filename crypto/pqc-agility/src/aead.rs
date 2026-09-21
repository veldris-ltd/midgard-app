//! AES-256-GCM authenticated encryption. One vetted implementation shared by the
//! whole platform: PQ-mTLS record protection in transit, and envelope encryption
//! at rest (KMS). No service re-rolls AEAD. [NFR-PQ-2] [NFR-SEC-1] [KDD-1]
//!
//! The bulk cipher is *not* post-quantum-sensitive: AES-256 has a 256-bit key, so
//! Grover's algorithm leaves an effective 128-bit security level. What must be
//! post-quantum is the *key agreement* that produces the key — handled by the
//! ML-KEM hybrid in [`crate::kem`]. Keys here are always 256-bit.

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, Key, KeyInit};

use crate::error::CryptoError;

/// AES-256 key length in bytes.
pub const AES256_KEY_LEN: usize = 32;
/// GCM nonce length in bytes (96-bit, the standard/most-efficient size).
pub const GCM_NONCE_LEN: usize = 12;
/// GCM authentication-tag length in bytes.
pub const GCM_TAG_LEN: usize = 16;

/// An AES-256-GCM AEAD keyed once and reused across many (nonce, msg) pairs.
///
/// **Nonce discipline is the caller's responsibility:** a (key, nonce) pair must
/// never protect two different plaintexts. Callers derive per-direction keys and
/// use a monotonic nonce counter (see the pq-mtls record layer).
pub struct Aes256GcmAead {
    cipher: Aes256Gcm,
}

impl Aes256GcmAead {
    /// Construct from a 256-bit key.
    pub fn new(key: &[u8; AES256_KEY_LEN]) -> Self {
        let key = Key::<Aes256Gcm>::from_slice(key);
        Self {
            cipher: Aes256Gcm::new(key),
        }
    }

    /// Seal `plaintext` under `nonce`, binding `aad`. Returns `ciphertext ‖ tag`
    /// (the tag is the trailing [`GCM_TAG_LEN`] bytes).
    pub fn seal(
        &self,
        nonce: &[u8; GCM_NONCE_LEN],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let nonce = GenericArray::from_slice(nonce);
        self.cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::AeadFailed)
    }

    /// Open `ciphertext ‖ tag` under `nonce`, checking `aad`. Fails closed with
    /// [`CryptoError::AeadFailed`] on any tag mismatch or tamper — the plaintext
    /// is never returned unless the tag verifies.
    pub fn open(
        &self,
        nonce: &[u8; GCM_NONCE_LEN],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let nonce = GenericArray::from_slice(nonce);
        self.cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::AeadFailed)
    }
}
