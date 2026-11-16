//! Keystore error type. Hand-rolled to keep the dependency surface minimal.
//! `[FR-C-2]`

use std::fmt;

/// An error from device key custody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeystoreError {
    /// Root keypair generation failed (e.g. the wrap suite is unavailable).
    RootGeneration(String),
    /// A brokered custody was asked to hold a key; use a key-holding keystore.
    BrokeredCannotHoldKey,
    /// Credential enrolment failed.
    Enrolment(String),
    /// The platform authenticator could not produce an assertion.
    AuthFailed(String),
    /// Authentication was denied (wrong credential or bad assertion). Fail closed.
    AuthDenied,
    /// No signature backend is available to authenticate.
    AuthBackendUnavailable,
    /// The user cancelled the platform prompt (Face ID / Touch ID / passcode).
    UserCancelled,
    /// The platform key root is unavailable (no Secure Enclave, key deleted, not
    /// enrolled) or did not seal what it was given.
    PlatformUnavailable(String),
    /// A sealed root blob is malformed (structure, magic or format version).
    Malformed(String),
    /// The sealed root did not open: another platform key, or tampered.
    Unseal,
}

impl fmt::Display for KeystoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeystoreError::RootGeneration(d) => write!(f, "keystore root generation failed: {d}"),
            KeystoreError::BrokeredCannotHoldKey => {
                write!(f, "a brokered keystore holds no key material")
            }
            KeystoreError::Enrolment(d) => write!(f, "credential enrolment failed: {d}"),
            KeystoreError::AuthFailed(d) => write!(f, "authentication failed: {d}"),
            KeystoreError::AuthDenied => write!(f, "authentication denied"),
            KeystoreError::AuthBackendUnavailable => {
                write!(f, "no signature backend available to authenticate")
            }
            KeystoreError::UserCancelled => write!(f, "the user cancelled authentication"),
            KeystoreError::PlatformUnavailable(d) => write!(f, "platform key unavailable: {d}"),
            KeystoreError::Malformed(d) => write!(f, "sealed root malformed: {d}"),
            KeystoreError::Unseal => write!(f, "sealed root did not open"),
        }
    }
}

impl std::error::Error for KeystoreError {}
