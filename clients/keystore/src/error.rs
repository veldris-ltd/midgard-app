//! Keystore error type. Hand-rolled to keep the dependency surface minimal.

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
        }
    }
}

impl std::error::Error for KeystoreError {}
