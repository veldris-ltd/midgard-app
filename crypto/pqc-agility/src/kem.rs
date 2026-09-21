//! Key-encapsulation abstraction, keyed by [`KemSuiteId`]. Hybrid combination is
//! internal to a backend's [`Kem`] implementation (see [`crate::hybrid`]); the
//! public artefacts carry the negotiated *suite* id so hot-swap needs no data
//! migration (AC-M-8).

use crate::algorithm::KemSuiteId;
use crate::error::CryptoError;
use crate::rng::Rng;
use crate::secret::{SecretArray, SecretBytes};

/// A KEM public key, tagged with its suite. Public data — cloneable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KemPublicKey {
    suite: KemSuiteId,
    bytes: Vec<u8>,
}

impl KemPublicKey {
    /// Construct from raw bytes.
    pub fn new(suite: KemSuiteId, bytes: Vec<u8>) -> Self {
        Self { suite, bytes }
    }
    /// The suite this key belongs to.
    pub fn suite(&self) -> KemSuiteId {
        self.suite
    }
    /// The raw key bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A KEM secret key, tagged with its suite. Zeroized on drop; not cloneable.
#[derive(Debug)]
pub struct KemSecretKey {
    suite: KemSuiteId,
    bytes: SecretBytes,
}

impl KemSecretKey {
    /// Construct from secret bytes.
    pub fn new(suite: KemSuiteId, bytes: SecretBytes) -> Self {
        Self { suite, bytes }
    }
    /// The suite this key belongs to.
    pub fn suite(&self) -> KemSuiteId {
        self.suite
    }
    /// Borrow the secret bytes.
    pub fn expose(&self) -> &[u8] {
        self.bytes.expose()
    }
}

/// A KEM ciphertext (encapsulation), tagged with its suite. Public — cloneable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ciphertext {
    suite: KemSuiteId,
    bytes: Vec<u8>,
}

impl Ciphertext {
    /// Construct from raw bytes.
    pub fn new(suite: KemSuiteId, bytes: Vec<u8>) -> Self {
        Self { suite, bytes }
    }
    /// The suite this ciphertext belongs to.
    pub fn suite(&self) -> KemSuiteId {
        self.suite
    }
    /// The raw ciphertext bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A 32-byte shared secret, zeroized on drop; not cloneable.
#[derive(Debug)]
pub struct SharedSecret(SecretArray<32>);

impl SharedSecret {
    /// Construct from raw bytes (crate-internal; produced by a backend).
    pub(crate) fn from_bytes(b: [u8; 32]) -> Self {
        SharedSecret(SecretArray::new(b))
    }
    /// Borrow the 32-byte secret.
    pub fn expose(&self) -> &[u8; 32] {
        self.0.expose()
    }
}

/// Declared byte lengths for a suite's artefacts (used for length checks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KemSizes {
    /// Public-key length.
    pub public_key: usize,
    /// Secret-key length.
    pub secret_key: usize,
    /// Ciphertext length.
    pub ciphertext: usize,
}

/// A key-encapsulation mechanism for one [`KemSuiteId`]. Object-safe.
pub trait Kem: Send + Sync {
    /// The suite implemented.
    fn suite(&self) -> KemSuiteId;
    /// Declared artefact sizes.
    fn sizes(&self) -> KemSizes;
    /// Generate a keypair.
    fn keypair(&self, rng: &mut dyn Rng) -> Result<(KemPublicKey, KemSecretKey), CryptoError>;
    /// Encapsulate to a public key, producing a ciphertext and shared secret.
    fn encapsulate(
        &self,
        pk: &KemPublicKey,
        rng: &mut dyn Rng,
    ) -> Result<(Ciphertext, SharedSecret), CryptoError>;
    /// Decapsulate a ciphertext with a secret key, recovering the shared secret.
    fn decapsulate(&self, sk: &KemSecretKey, ct: &Ciphertext) -> Result<SharedSecret, CryptoError>;
}
