//! # kms — Crypto & Key-Management (envelope encryption)
//!
//! Data at rest is **AES-256-GCM under a key wrapped by ML-KEM-1024**, and the
//! wrapping key material is **sealed to the trust root**. [NFR-PQ-2] [AC-M-2]
//!
//! The scheme is standard KEM-DEM envelope encryption:
//! - **Seal:** encapsulate to the trust root's ML-KEM-1024 public key to obtain a
//!   fresh shared secret; derive a 256-bit data-encryption key (DEK) from it via
//!   HKDF; AES-256-GCM-encrypt the payload under the DEK. The KEM ciphertext *is*
//!   the wrapped key — only the trust root can unwrap it.
//! - **Unseal:** the trust root decapsulates the KEM ciphertext (in software, or
//!   in an HSM/TPM where present) to recover the shared secret; re-derive the DEK;
//!   AES-256-GCM-decrypt. A tampered blob or the wrong trust root fails closed.
//!
//! The DEK is ephemeral and never stored; the payload is bound to the root by the
//! encapsulation. Rotating the root re-wraps new data without touching old blobs
//! (each blob records its root key id and suite → crypto-agility, no migration).

use audit_writer::{AuditError, Sealer};
use pqc_agility::hash::hkdf_sha256_32;
use pqc_agility::{
    Aes256GcmAead, Ciphertext, Kem, KemPublicKey, KemSecretKey, KemSuiteId, OsRng, Rng,
    SharedSecret, GCM_NONCE_LEN,
};

mod error;
#[cfg(feature = "backend-fips")]
pub mod hardware;
pub mod rotation;
pub mod signing;

pub use error::KmsError;
#[cfg(feature = "backend-fips")]
pub use hardware::{HardwareToken, HardwareTrustRoot, RootAttestation, SoftHsmToken};
pub use rotation::RotatingKms;
pub use signing::{SigningRoot, SoftwareSigningRoot, RELEASE_SIG_ALG};

/// The pure ML-KEM-1024 suite used to wrap data keys at rest. [NFR-PQ-2]
pub const WRAP_SUITE: KemSuiteId = KemSuiteId::MlKem1024;

const KMS_HKDF_SALT: &[u8] = b"midgard-kms-v1";
const KMS_HKDF_INFO: &[u8] = b"data-encryption-key";

/// Where the root key material lives and where unwrapping happens. A software
/// implementation holds the key in memory; an HSM/TPM implementation performs the
/// decapsulation in hardware and never exposes the secret. [NFR-PQ-2]
pub trait TrustRoot {
    /// Stable identifier of this root key (recorded in every sealed blob).
    fn key_id(&self) -> &str;
    /// The root's ML-KEM public key, encapsulated to when sealing.
    fn public_key(&self) -> &KemPublicKey;
    /// Unwrap a KEM ciphertext to its shared secret using the sealed root key.
    fn unwrap(&self, kem: &dyn Kem, wrapped: &Ciphertext) -> Result<SharedSecret, KmsError>;
}

/// A software-sealed trust root: the root ML-KEM-1024 keypair held in process.
/// The HSM/TPM seam is [`TrustRoot`] itself — a hardware root implements the same
/// trait with the secret key never leaving the device.
pub struct SoftwareTrustRoot {
    key_id: String,
    public_key: KemPublicKey,
    secret_key: KemSecretKey,
}

impl SoftwareTrustRoot {
    /// Generate a fresh root keypair for the wrap suite.
    pub fn generate(
        provider: &pqc_agility::CryptoProvider,
        key_id: &str,
        rng: &mut dyn Rng,
    ) -> Result<Self, KmsError> {
        let kem = provider
            .kem_for(WRAP_SUITE)
            .map_err(|_| KmsError::SuiteUnavailable)?;
        let (public_key, secret_key) =
            kem.keypair(rng).map_err(|e| KmsError::Kem(e.to_string()))?;
        Ok(Self {
            key_id: key_id.to_string(),
            public_key,
            secret_key,
        })
    }
}

impl TrustRoot for SoftwareTrustRoot {
    fn key_id(&self) -> &str {
        &self.key_id
    }
    fn public_key(&self) -> &KemPublicKey {
        &self.public_key
    }
    fn unwrap(&self, kem: &dyn Kem, wrapped: &Ciphertext) -> Result<SharedSecret, KmsError> {
        kem.decapsulate(&self.secret_key, wrapped)
            .map_err(|e| KmsError::Kem(e.to_string()))
    }
}

/// An at-rest sealed blob. Self-describing so it can be unsealed by any KMS that
/// holds the named trust root, across algorithm rotations. Carries no plaintext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedBlob {
    /// The trust-root key id this blob is sealed to.
    pub root_key_id: String,
    /// The KEM suite used to wrap the data key.
    pub suite: KemSuiteId,
    /// The ML-KEM-1024 encapsulation — the wrapped data key. [NFR-PQ-2]
    pub wrapped_key: Ciphertext,
    /// AES-GCM nonce.
    pub nonce: [u8; GCM_NONCE_LEN],
    /// AES-256-GCM ciphertext (payload ‖ tag).
    pub ciphertext: Vec<u8>,
}

impl SealedBlob {
    /// Serialize to self-describing bytes for durable storage (e.g. an audit
    /// [`FileLedger`](audit_writer::FileLedger) frame). No serde.
    pub fn encode(&self) -> Vec<u8> {
        fn put(o: &mut Vec<u8>, b: &[u8]) {
            o.extend_from_slice(&(b.len() as u32).to_le_bytes());
            o.extend_from_slice(b);
        }
        let mut o = Vec::new();
        put(&mut o, self.root_key_id.as_bytes());
        o.push(self.suite.as_u8());
        put(&mut o, self.wrapped_key.as_bytes());
        o.extend_from_slice(&self.nonce);
        put(&mut o, &self.ciphertext);
        o
    }

    /// Parse bytes produced by [`SealedBlob::encode`]. Fails closed on truncation,
    /// an unknown suite tag, or trailing bytes.
    pub fn decode(b: &[u8]) -> Result<SealedBlob, KmsError> {
        fn rd_len<'a>(b: &'a [u8], pos: &mut usize) -> Result<&'a [u8], KmsError> {
            if *pos + 4 > b.len() {
                return Err(KmsError::Malformed("truncated length".into()));
            }
            let n = u32::from_le_bytes([b[*pos], b[*pos + 1], b[*pos + 2], b[*pos + 3]]) as usize;
            *pos += 4;
            let end = pos
                .checked_add(n)
                .ok_or_else(|| KmsError::Malformed("length overflow".into()))?;
            if end > b.len() {
                return Err(KmsError::Malformed("truncated body".into()));
            }
            let s = &b[*pos..end];
            *pos = end;
            Ok(s)
        }
        let mut pos = 0usize;
        let root_key_id = String::from_utf8(rd_len(b, &mut pos)?.to_vec())
            .map_err(|_| KmsError::Malformed("root id not utf8".into()))?;
        if pos >= b.len() {
            return Err(KmsError::Malformed("missing suite".into()));
        }
        let suite = KemSuiteId::from_u8(b[pos])
            .ok_or_else(|| KmsError::Malformed("unknown suite".into()))?;
        pos += 1;
        let wrapped = rd_len(b, &mut pos)?.to_vec();
        if pos + GCM_NONCE_LEN > b.len() {
            return Err(KmsError::Malformed("truncated nonce".into()));
        }
        let mut nonce = [0u8; GCM_NONCE_LEN];
        nonce.copy_from_slice(&b[pos..pos + GCM_NONCE_LEN]);
        pos += GCM_NONCE_LEN;
        let ciphertext = rd_len(b, &mut pos)?.to_vec();
        if pos != b.len() {
            return Err(KmsError::Malformed("trailing bytes".into()));
        }
        Ok(SealedBlob {
            root_key_id,
            suite,
            wrapped_key: Ciphertext::new(suite, wrapped),
            nonce,
            ciphertext,
        })
    }
}

/// The key-management service: seals and unseals payloads against a trust root.
pub struct Kms<R: TrustRoot> {
    provider: pqc_agility::CryptoProvider,
    root: R,
}

impl<R: TrustRoot> Kms<R> {
    /// Build a KMS over a provider and a trust root.
    pub fn new(provider: pqc_agility::CryptoProvider, root: R) -> Self {
        Self { provider, root }
    }

    /// The trust root's key id.
    pub fn root_key_id(&self) -> &str {
        self.root.key_id()
    }

    /// Seal `plaintext` (binding `aad`) at rest. The result contains no plaintext
    /// and can only be unsealed by the holder of the trust root. [AC-M-2]
    pub fn seal(
        &self,
        aad: &[u8],
        plaintext: &[u8],
        rng: &mut dyn Rng,
    ) -> Result<SealedBlob, KmsError> {
        seal_to_root(&self.provider, &self.root, aad, plaintext, rng)
    }

    /// Unseal a blob. Fails closed on the wrong root, a suite mismatch, a tampered
    /// wrapped key, or a tampered ciphertext.
    pub fn unseal(&self, aad: &[u8], blob: &SealedBlob) -> Result<Vec<u8>, KmsError> {
        if blob.root_key_id != self.root.key_id() {
            return Err(KmsError::WrongTrustRoot);
        }
        unseal_from_root(&self.provider, &self.root, aad, blob)
    }
}

/// Derive the ephemeral data-encryption key from a KEM shared secret.
fn derive_dek(ss: &SharedSecret) -> [u8; 32] {
    hkdf_sha256_32(KMS_HKDF_SALT, ss.expose(), KMS_HKDF_INFO)
}

/// Envelope-seal `plaintext` to `root` (encapsulate to its public key, derive a DEK,
/// AES-256-GCM the payload). Shared by [`Kms`] and [`RotatingKms`]. [NFR-PQ-2]
pub fn seal_to_root(
    provider: &pqc_agility::CryptoProvider,
    root: &dyn TrustRoot,
    aad: &[u8],
    plaintext: &[u8],
    rng: &mut dyn Rng,
) -> Result<SealedBlob, KmsError> {
    let kem = provider
        .kem_for(WRAP_SUITE)
        .map_err(|_| KmsError::SuiteUnavailable)?;
    let (wrapped_key, ss) = kem
        .encapsulate(root.public_key(), rng)
        .map_err(|e| KmsError::Kem(e.to_string()))?;
    let dek = derive_dek(&ss);
    let mut nonce = [0u8; GCM_NONCE_LEN];
    rng.fill(&mut nonce);
    let ciphertext = Aes256GcmAead::new(&dek)
        .seal(&nonce, aad, plaintext)
        .map_err(|_| KmsError::Aead)?;
    Ok(SealedBlob {
        root_key_id: root.key_id().to_string(),
        suite: WRAP_SUITE,
        wrapped_key,
        nonce,
        ciphertext,
    })
}

/// Envelope-unseal `blob` with `root` (the root unwraps the KEM ciphertext — in an
/// HSM/TPM the secret never leaves the device — then re-derive the DEK and open).
/// The caller must have already checked `blob.root_key_id` matches `root`.
pub fn unseal_from_root(
    provider: &pqc_agility::CryptoProvider,
    root: &dyn TrustRoot,
    aad: &[u8],
    blob: &SealedBlob,
) -> Result<Vec<u8>, KmsError> {
    if blob.suite != WRAP_SUITE {
        return Err(KmsError::SuiteMismatch);
    }
    let kem = provider
        .kem_for(WRAP_SUITE)
        .map_err(|_| KmsError::SuiteUnavailable)?;
    let ss = root.unwrap(kem, &blob.wrapped_key)?;
    let dek = derive_dek(&ss);
    Aes256GcmAead::new(&dek)
        .open(&blob.nonce, aad, &blob.ciphertext)
        .map_err(|_| KmsError::Aead)
}

/// A [`Sealer`](audit_writer::Sealer) backed by this KMS: it seals opaque bytes at
/// rest with the envelope scheme (AES-256-GCM under an ML-KEM-1024-wrapped key,
/// sealed to the trust root). This is how the durable audit
/// [`FileLedger`](audit_writer::FileLedger) encrypts every entry. [NFR-PQ-2] [AC-M-4]
pub struct KmsSealer<R: TrustRoot> {
    kms: Kms<R>,
}

impl<R: TrustRoot> KmsSealer<R> {
    /// Wrap a KMS as a sealer.
    pub fn new(kms: Kms<R>) -> Self {
        Self { kms }
    }
    /// Borrow the underlying KMS.
    pub fn kms(&self) -> &Kms<R> {
        &self.kms
    }
}

impl<R: TrustRoot + Send + Sync> Sealer for KmsSealer<R> {
    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
        // Production randomness from the OS CSPRNG (a syscall — offline). [NFR-PRV-1]
        let mut rng = OsRng;
        let blob = self
            .kms
            .seal(aad, plaintext, &mut rng)
            .map_err(|e| AuditError::Store(format!("kms seal: {e}")))?;
        Ok(blob.encode())
    }
    fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
        let blob = SealedBlob::decode(sealed)
            .map_err(|e| AuditError::Store(format!("kms decode: {e}")))?;
        self.kms
            .unseal(aad, &blob)
            .map_err(|e| AuditError::Store(format!("kms unseal: {e}")))
    }
}
