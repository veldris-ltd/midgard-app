//! Operational key rotation. A [`RotatingKms`] holds a keyring of trust roots — one
//! **current** root plus any number of **retired** roots kept for unsealing. New
//! data is always sealed to the current root; existing blobs are unsealed by
//! dispatching to the root whose `root_key_id` they record. Rotating therefore
//! **re-wraps new data without touching old blobs** — no bulk re-encryption, no
//! migration window. Old blobs stay valid until their root is retired *and* dropped.
//! [NFR-PQ-2] [RC-28]
//!
//! Each root may be software ([`SoftwareTrustRoot`](crate::SoftwareTrustRoot)) or
//! hardware ([`HardwareTrustRoot`](crate::hardware::HardwareTrustRoot)); the keyring
//! is uniform over the [`TrustRoot`](crate::TrustRoot) trait, so rotating from a
//! software root to a hardware root is just an `add_root` + `set_current`.

use pqc_agility::{CryptoProvider, Rng};

use crate::error::KmsError;
use crate::{seal_to_root, unseal_from_root, SealedBlob, TrustRoot};

/// A KMS with a rotating keyring of trust roots.
pub struct RotatingKms {
    provider: CryptoProvider,
    roots: Vec<Box<dyn TrustRoot>>,
    current: usize,
}

impl RotatingKms {
    /// Start a keyring with `initial` as the sole (current) root.
    pub fn new(provider: CryptoProvider, initial: Box<dyn TrustRoot>) -> Self {
        Self {
            provider,
            roots: vec![initial],
            current: 0,
        }
    }

    /// The current root's key id (what new seals are bound to).
    pub fn current_key_id(&self) -> &str {
        self.roots[self.current].key_id()
    }

    /// The ids of every root the keyring can still unseal with.
    pub fn known_root_ids(&self) -> Vec<&str> {
        self.roots.iter().map(|r| r.key_id()).collect()
    }

    /// Add a root to the keyring **and make it current**. Prior roots are retained
    /// so their blobs still unseal. This is the rotation step: after it, new seals
    /// use the new root; nothing already at rest is rewritten. Rejects a duplicate
    /// key id. [RC-28]
    pub fn rotate_to(&mut self, new_root: Box<dyn TrustRoot>) -> Result<(), KmsError> {
        let id = new_root.key_id().to_string();
        if self.roots.iter().any(|r| r.key_id() == id) {
            return Err(KmsError::HardwareUnavailable(format!(
                "root id '{id}' already in the keyring"
            )));
        }
        self.roots.push(new_root);
        self.current = self.roots.len() - 1;
        Ok(())
    }

    /// Retire (drop) a root by id — its blobs can no longer be unsealed afterwards.
    /// Refuses to retire the current root. Operators run this only once all data
    /// under that root has been re-sealed or is expired. [RC-28]
    pub fn retire(&mut self, key_id: &str) -> Result<(), KmsError> {
        if self.current_key_id() == key_id {
            return Err(KmsError::HardwareUnavailable(
                "cannot retire the current root".into(),
            ));
        }
        let before = self.roots.len();
        let current_id = self.current_key_id().to_string();
        self.roots.retain(|r| r.key_id() != key_id);
        if self.roots.len() == before {
            return Err(KmsError::UnknownRoot(key_id.to_string()));
        }
        // The current index may have shifted after removal — recompute it.
        self.current = self
            .roots
            .iter()
            .position(|r| r.key_id() == current_id)
            .expect("current root retained");
        Ok(())
    }

    /// Seal `plaintext` to the **current** root.
    pub fn seal(
        &self,
        aad: &[u8],
        plaintext: &[u8],
        rng: &mut dyn Rng,
    ) -> Result<SealedBlob, KmsError> {
        seal_to_root(
            &self.provider,
            &*self.roots[self.current],
            aad,
            plaintext,
            rng,
        )
    }

    /// Unseal `blob` by dispatching to the root it was sealed to. Fails closed if no
    /// known root matches (e.g. a retired-and-dropped root). Old blobs unseal with
    /// their original root untouched. [RC-28]
    pub fn unseal(&self, aad: &[u8], blob: &SealedBlob) -> Result<Vec<u8>, KmsError> {
        let root = self
            .roots
            .iter()
            .find(|r| r.key_id() == blob.root_key_id)
            .ok_or_else(|| KmsError::UnknownRoot(blob.root_key_id.clone()))?;
        unseal_from_root(&self.provider, &**root, aad, blob)
    }
}
