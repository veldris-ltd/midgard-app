//! A custody root whose secret is sealed at rest by a platform key root. `[FR-C-2]`
//! `[NFR-PQ-2]` `[VLD-ANB-MIDGARD-0056]`
//!
//! Platform key stores (the Apple Secure Enclave, Android StrongBox) hold P-256 keys
//! and cannot hold or decapsulate ML-KEM-1024. So the ML-KEM root is generated here,
//! and at rest its secret is AES-256-GCM-encrypted under a 32-byte **data key** that
//! only the platform key root can unseal. The one thing that crosses to the platform
//! side ([`RootSealer`]) is that data key: sealed once when the root is made, unsealed
//! once per unlock. The ML-KEM secret never leaves this crate, and the platform key
//! never leaves its hardware. This crate offers no accessor for either secret.
//!
//! On Apple the platform side (Tasks A11/A13) seals with the Secure Enclave key
//! (`SecKeyCreateEncryptedData`, `eciesEncryptionCofactorVariableIVX963SHA256AESGCM`) and
//! unseals with `SecKeyCreateDecryptedData`, which the key's access control gates on
//! Face ID / Touch ID with the passcode as fallback.
//!
//! ```text
//! magic 8 "MGSROOT1" | key_id 4+n | suite 1 | root_public 4+n | sealed_data_key 4+n
//! | nonce 12 | ciphertext 4+n          (AAD = everything before the nonce)
//! ```
//!
//! The version is the magic's last byte; another version is refused as malformed.

use kms::{KmsError, TrustRoot, WRAP_SUITE};
use pqc_agility::{
    hkdf_sha256_32, Aes256GcmAead, Ciphertext, CryptoProvider, Kem, KemPublicKey, KemSecretKey,
    KemSuiteId, Rng, SecretBytes, SharedSecret, Zeroizing, GCM_NONCE_LEN,
};

use crate::{Keystore, KeystoreError, SecurityFloor};

/// File magic and format version.
pub const MAGIC: &[u8; 8] = b"MGSROOT1";
/// The length of the data key the platform side seals.
pub const DATA_KEY_LEN: usize = 32;
const KEK_SALT: &[u8] = b"midgard-client/sealed-root";
const KEK_INFO: &[u8] = b"midgard-client/sealed-root/v1/root-secret";

/// The platform key root: seals and unseals the data key, and nothing else.
///
/// This is the callback the Swift Secure Enclave implementation provides. It is
/// given the data key to seal and gives it back on unseal; it is never given the
/// ML-KEM root secret, and no method returns key material to the platform side.
pub trait RootSealer {
    /// Seal `data_key` to the platform key, creating that key first if it does not
    /// exist. Needs no user presence (the Enclave's public half encrypts).
    fn seal(&self, data_key: &SecretBytes) -> Result<Vec<u8>, KeystoreError>;
    /// Unseal what [`seal`](Self::seal) returned. Prompts for user presence; a
    /// cancelled prompt is [`KeystoreError::UserCancelled`].
    fn unseal(&self, sealed: &[u8]) -> Result<SecretBytes, KeystoreError>;
    /// Whether the platform key is held in hardware (Secure Enclave). The simulator
    /// and tests use a software platform key and say so.
    fn hardware_backed(&self) -> bool;
}

/// An ML-KEM-1024 root, sealed at rest under a data key a [`RootSealer`] holds.
pub struct SealedRootKeystore {
    key_id: String,
    public_key: KemPublicKey,
    secret: KemSecretKey,
    hardware_wrapped: bool,
}

fn put(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

fn header(key_id: &str, public: &KemPublicKey, sealed_key: &[u8]) -> Vec<u8> {
    let mut o = MAGIC.to_vec();
    put(&mut o, key_id.as_bytes());
    o.push(public.suite().as_u8());
    put(&mut o, public.as_bytes());
    put(&mut o, sealed_key);
    o
}

fn kek(data_key: &SecretBytes) -> Zeroizing<[u8; 32]> {
    Zeroizing::new(hkdf_sha256_32(KEK_SALT, data_key.expose(), KEK_INFO))
}

/// A bounds-checked reader: `None` on any short read, never a panic.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len())?;
        let s = &self.b[self.at..end];
        self.at = end;
        Some(s)
    }
    fn field(&mut self) -> Option<&'a [u8]> {
        let n = u32::from_le_bytes(self.take(4)?.try_into().ok()?) as usize;
        self.take(n)
    }
}

impl SealedRootKeystore {
    /// Generate a root and seal it. Returns the keystore and the blob to store (it
    /// holds no secret in the clear). Asks the platform side to seal once.
    pub fn create(
        provider: &CryptoProvider,
        key_id: &str,
        sealer: &dyn RootSealer,
        rng: &mut dyn Rng,
    ) -> Result<(Self, Vec<u8>), KeystoreError> {
        let kem = provider
            .kem_for(WRAP_SUITE)
            .map_err(|e| KeystoreError::RootGeneration(e.to_string()))?;
        let (public_key, secret) = kem
            .keypair(rng)
            .map_err(|e| KeystoreError::RootGeneration(e.to_string()))?;
        let mut raw = Zeroizing::new([0u8; DATA_KEY_LEN]);
        rng.fill(&mut raw[..]);
        let data_key = SecretBytes::new(raw.to_vec());
        let sealed_key = sealer.seal(&data_key)?;
        // A platform side that hands the key back in the clear has not sealed it.
        if sealed_key.len() < DATA_KEY_LEN
            || sealed_key
                .windows(DATA_KEY_LEN)
                .any(|w| w == data_key.expose())
        {
            return Err(KeystoreError::PlatformUnavailable(
                "the platform key root did not seal the data key".into(),
            ));
        }
        let mut blob = header(key_id, &public_key, &sealed_key);
        let mut nonce = [0u8; GCM_NONCE_LEN];
        rng.fill(&mut nonce);
        let ct = Aes256GcmAead::new(&kek(&data_key))
            .seal(&nonce, &blob, secret.expose())
            .map_err(|e| KeystoreError::RootGeneration(e.to_string()))?;
        blob.extend_from_slice(&nonce);
        put(&mut blob, &ct);
        let ks = Self {
            key_id: key_id.to_string(),
            public_key,
            secret,
            hardware_wrapped: sealer.hardware_backed(),
        };
        Ok((ks, blob))
    }

    /// Reopen a sealed root through the platform key root that sealed it. The blob's
    /// structure is checked in full before the platform side is asked (so a
    /// malformed blob never prompts); the data key is then unsealed once.
    /// [`KeystoreError::Malformed`] is structure or version; [`KeystoreError::Unseal`]
    /// is another platform key or a changed blob.
    pub fn open(blob: &[u8], sealer: &dyn RootSealer) -> Result<Self, KeystoreError> {
        let bad = |d: &str| KeystoreError::Malformed(d.to_string());
        let mut c = Cursor { b: blob, at: 0 };
        let magic = c.take(MAGIC.len()).ok_or_else(|| bad("truncated"))?;
        if magic[..7] != MAGIC[..7] {
            return Err(bad("not a sealed root"));
        }
        if magic != MAGIC {
            return Err(bad("unsupported format version"));
        }
        let key_id = String::from_utf8(c.field().ok_or_else(|| bad("key id"))?.to_vec())
            .map_err(|_| bad("key id"))?;
        let suite = c
            .take(1)
            .and_then(|b| KemSuiteId::from_u8(b[0]))
            .filter(|s| *s == WRAP_SUITE)
            .ok_or_else(|| bad("suite"))?;
        let public = c.field().ok_or_else(|| bad("public key"))?.to_vec();
        let sealed_key = c.field().ok_or_else(|| bad("sealed data key"))?;
        let header_len = c.at;
        let nonce: [u8; GCM_NONCE_LEN] = c
            .take(GCM_NONCE_LEN)
            .and_then(|n| n.try_into().ok())
            .ok_or_else(|| bad("nonce"))?;
        let ct = c.field().ok_or_else(|| bad("ciphertext"))?;
        if c.at != blob.len() {
            return Err(bad("trailing bytes"));
        }
        let data_key = sealer.unseal(sealed_key)?;
        if data_key.len() != DATA_KEY_LEN {
            return Err(KeystoreError::Unseal);
        }
        let secret = Aes256GcmAead::new(&kek(&data_key))
            .open(&nonce, &blob[..header_len], ct)
            .map_err(|_| KeystoreError::Unseal)?;
        Ok(Self {
            key_id,
            public_key: KemPublicKey::new(suite, public),
            secret: KemSecretKey::new(suite, SecretBytes::new(secret)),
            hardware_wrapped: sealer.hardware_backed(),
        })
    }

    /// Whether the data key is sealed by a hardware-held platform key (the custody
    /// chip says "Secure Enclave–wrapped"). It does not raise the floor.
    pub fn hardware_wrapped(&self) -> bool {
        self.hardware_wrapped
    }
}

impl Keystore for SealedRootKeystore {
    /// Always [`SecurityFloor::NonExtractable`], whatever the platform key is.
    ///
    /// Owner decision 5 (16 Nov 2026, `docs/plans/2026-11-13-apple-clients-plan.md`):
    /// accept `NonExtractable` for the Apple key root, with no new custody level. The
    /// reason: the Secure Enclave cannot hold or decapsulate ML-KEM-1024, so while
    /// the app is unlocked the root secret is in (mlocked) process memory, exactly as
    /// [`SoftwareKeystore`](crate::SoftwareKeystore)'s is; the Enclave binds it only
    /// at rest. Claiming [`SecurityFloor::HardwareBacked`] would overstate custody,
    /// so this root does not meet `PlatformProfile::Apple.floor()` and the Apple apps
    /// open their store at `NonExtractable` (a recorded deviation from SAD_06 §11.2).
    /// "The floor is honest." `[VLD-ANB-MIDGARD-0056]`
    fn floor(&self) -> SecurityFloor {
        SecurityFloor::NonExtractable
    }
    fn holds_key(&self) -> bool {
        true
    }
    fn key_id(&self) -> &str {
        &self.key_id
    }
}

impl TrustRoot for SealedRootKeystore {
    fn key_id(&self) -> &str {
        &self.key_id
    }
    fn public_key(&self) -> &KemPublicKey {
        &self.public_key
    }
    fn unwrap(&self, kem: &dyn Kem, wrapped: &Ciphertext) -> Result<SharedSecret, KmsError> {
        kem.decapsulate(&self.secret, wrapped)
            .map_err(|e| KmsError::Kem(e.to_string()))
    }
}
