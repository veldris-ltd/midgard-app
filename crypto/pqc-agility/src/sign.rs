//! Digital-signature abstraction. `verify` returns `Result<(), _>` (never a
//! `bool`) so a caller cannot accidentally treat an error path as success.

use crate::algorithm::SigId;
use crate::error::CryptoError;
use crate::rng::Rng;
use crate::secret::SecretBytes;

/// A verifying (public) key, tagged with its algorithm. Public — cloneable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyingKey {
    alg: SigId,
    bytes: Vec<u8>,
}

impl VerifyingKey {
    /// Construct from raw bytes.
    pub fn new(alg: SigId, bytes: Vec<u8>) -> Self {
        Self { alg, bytes }
    }
    /// The algorithm.
    pub fn alg(&self) -> SigId {
        self.alg
    }
    /// The raw key bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A signing (secret) key, zeroized on drop; not cloneable.
#[derive(Debug)]
pub struct SigningKey {
    alg: SigId,
    bytes: SecretBytes,
}

impl SigningKey {
    /// Construct from secret bytes.
    pub fn new(alg: SigId, bytes: SecretBytes) -> Self {
        Self { alg, bytes }
    }
    /// The algorithm.
    pub fn alg(&self) -> SigId {
        self.alg
    }
    /// Borrow the secret bytes.
    pub fn expose(&self) -> &[u8] {
        self.bytes.expose()
    }
}

/// A signature, tagged with the algorithm that produced it. Public — cloneable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    alg: SigId,
    bytes: Vec<u8>,
}

impl Signature {
    /// Construct from raw bytes.
    pub fn new(alg: SigId, bytes: Vec<u8>) -> Self {
        Self { alg, bytes }
    }
    /// The algorithm.
    pub fn alg(&self) -> SigId {
        self.alg
    }
    /// The raw signature bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A signature scheme for one [`SigId`]. Object-safe.
pub trait SignatureScheme: Send + Sync {
    /// The algorithm implemented.
    fn id(&self) -> SigId;
    /// Generate a keypair.
    fn keypair(&self, rng: &mut dyn Rng) -> Result<(VerifyingKey, SigningKey), CryptoError>;
    /// Sign a message.
    fn sign(&self, sk: &SigningKey, msg: &[u8]) -> Result<Signature, CryptoError>;
    /// Verify a signature; `Ok(())` iff valid.
    fn verify(&self, vk: &VerifyingKey, msg: &[u8], sig: &Signature) -> Result<(), CryptoError>;
}
