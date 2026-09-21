//! Local-store error type.

use std::fmt;

use midgard_client_keystore::SecurityFloor;

/// An error from the local encrypted cache.
#[derive(Debug)]
pub enum StoreError {
    /// The keystore holds no local key (a brokered/extension custody).
    NoLocalKey,
    /// The keystore's custody floor is below the required minimum.
    FloorNotMet {
        /// The minimum floor the store demanded.
        required: SecurityFloor,
        /// The floor the keystore actually provides.
        actual: SecurityFloor,
    },
    /// The object's marking is not dominated by the session clearance: it is
    /// neither cached nor rendered. `[AC-C-13]`
    AboveClearance,
    /// No such object is cached.
    NotFound,
    /// An at-rest seal/unseal failed (tamper, wrong root, suite mismatch).
    Kms(kms::KmsError),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NoLocalKey => write!(f, "keystore holds no local key (brokered custody)"),
            StoreError::FloorNotMet { required, actual } => write!(
                f,
                "keystore floor {actual:?} is below the required minimum {required:?}"
            ),
            StoreError::AboveClearance => {
                write!(f, "object marking exceeds session clearance; not cached")
            }
            StoreError::NotFound => write!(f, "object not cached"),
            StoreError::Kms(e) => write!(f, "at-rest seal/unseal failed: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<kms::KmsError> for StoreError {
    fn from(e: kms::KmsError) -> Self {
        StoreError::Kms(e)
    }
}
