//! The platform key root the core calls back: it seals and unseals the core's 32-byte
//! data key with a key held in the Secure Enclave (Apple) or StrongBox (Android). The
//! shell implements this trait and nothing else that touches keys; it never sees the
//! ML-KEM root secret or the device-audit key, and no method of this crate returns
//! key material to it. `[FR-C-2]` `[DD-6]` `[NFR-PQ-2]`

use std::sync::Arc;

use midgard_client_core::keystore::sealed_root::RootSealer;
use midgard_client_core::keystore::KeystoreError;
use pqc_agility::SecretBytes;

/// Why the platform key root could not answer.
#[derive(Debug, uniffi::Error)]
pub enum KeyRootError {
    /// The user cancelled Face ID / Touch ID / the passcode.
    UserCancelled,
    /// No usable platform key (no Enclave, biometry not enrolled, key deleted), or the
    /// platform call failed.
    PlatformUnavailable {
        /// The platform's message.
        detail: String,
    },
    /// The sealed bytes are not something the platform key could have sealed
    /// (wrong length or structure: corrupt).
    Malformed {
        /// What.
        detail: String,
    },
    /// The sealed bytes did not open under the platform key (another key, or
    /// tampered).
    Unseal,
}

impl std::fmt::Display for KeyRootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyRootError::UserCancelled => write!(f, "cancelled"),
            KeyRootError::PlatformUnavailable { detail } => {
                write!(f, "platform key unavailable: {detail}")
            }
            KeyRootError::Malformed { detail } => write!(f, "sealed key malformed: {detail}"),
            KeyRootError::Unseal => write!(f, "sealed key did not open"),
        }
    }
}

impl std::error::Error for KeyRootError {}

impl From<uniffi::UnexpectedUniFFICallbackError> for KeyRootError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        KeyRootError::PlatformUnavailable { detail: e.reason }
    }
}

/// The platform key root, implemented in Swift by `SecureEnclaveKeyRoot`.
///
/// `seal` is given the data key to seal (it needs no user presence: the Enclave's
/// public half encrypts) and creates the platform key first if there is none;
/// `unseal` gives it back (it prompts). Neither is ever given anything else.
#[uniffi::export(with_foreign)]
pub trait KeyRoot: Send + Sync {
    /// Seal `data_key` (32 bytes) to the platform key.
    fn seal(&self, data_key: Vec<u8>) -> Result<Vec<u8>, KeyRootError>;
    /// Unseal what [`seal`](Self::seal) returned (prompts for user presence).
    fn unseal(&self, sealed: Vec<u8>) -> Result<Vec<u8>, KeyRootError>;
    /// Whether the platform key is held in hardware.
    fn hardware_backed(&self) -> bool;
}

impl From<KeyRootError> for KeystoreError {
    fn from(e: KeyRootError) -> Self {
        match e {
            KeyRootError::UserCancelled => KeystoreError::UserCancelled,
            KeyRootError::PlatformUnavailable { detail } => {
                KeystoreError::PlatformUnavailable(detail)
            }
            KeyRootError::Malformed { detail } => KeystoreError::Malformed(detail),
            KeyRootError::Unseal => KeystoreError::Unseal,
        }
    }
}

/// Adapts the foreign [`KeyRoot`] to the keystore's [`RootSealer`].
pub(crate) struct Sealer(pub Arc<dyn KeyRoot>);

impl RootSealer for Sealer {
    fn seal(&self, data_key: &SecretBytes) -> Result<Vec<u8>, KeystoreError> {
        // The copy handed across is consumed by the binding; the platform side
        // wipes its own copy after sealing (Task A13).
        Ok(self.0.seal(data_key.expose().to_vec())?)
    }

    fn unseal(&self, sealed: &[u8]) -> Result<SecretBytes, KeystoreError> {
        // Into mlocked, zeroised memory at once.
        Ok(SecretBytes::new(self.0.unseal(sealed.to_vec())?))
    }

    fn hardware_backed(&self) -> bool {
        self.0.hardware_backed()
    }
}
