//! Typed errors for the shells: each variant is a screen state, never a crash.
//! `[FR-C-1]` `[AC-C-13]` `[FR-C-4]`

use midgard_client_core::demo::dataset::DatasetError;
use midgard_client_core::device_audit::DeviceKeyError;
use midgard_client_core::keystore::KeystoreError;
use midgard_client_core::session::SessionError;
use midgard_client_core::source::SourceError;
use midgard_client_core::store::StoreError;
use midgard_client_core::CoreError;

/// An error a binding returns.
#[derive(Debug, uniffi::Error)]
pub enum ClientError {
    /// Above the session's clearance (or not in its view): shown as a withheld row
    /// or a refused action.
    AboveClearance,
    /// The source is unreachable (or the demo is offline).
    Offline,
    /// Offline (or never read) and nothing is cached for this collection.
    NothingCached {
        /// The collection.
        collection: String,
    },
    /// A persona switch while actions are unsent: sync first.
    PendingOps {
        /// How many ops.
        count: u32,
    },
    /// An action target that is not `<collection>/<row id>`.
    BadTarget {
        /// The target given.
        target: String,
    },
    /// A state file older than the newest the shell kept for this persona (a
    /// rollback). Nothing was loaded: discard the file and refetch.
    Stale {
        /// The file's generation.
        found: u64,
        /// The lowest generation accepted.
        min: u64,
    },
    /// A name the core reserves for itself.
    Reserved {
        /// The name.
        what: String,
    },
    /// The device-audit key did not open (another root, a changed blob) or sign.
    DeviceKey {
        /// What failed.
        detail: String,
    },
    /// The bundled dataset failed its signature check under the pinned anchor.
    SignatureFailure {
        /// What failed.
        detail: String,
    },
    /// Sealed data did not open (state file sealed to another root or persona, or
    /// changed).
    Integrity {
        /// What failed.
        detail: String,
    },
    /// Malformed data (a dataset body, a row, a state file, a sync answer).
    Malformed {
        /// What.
        detail: String,
    },
    /// The client is locked: unlock first.
    Locked,
    /// Already unlocked: lock (or switch persona) first.
    AlreadyUnlocked,
    /// Another unlock is waiting on the platform prompt.
    UnlockInProgress,
    /// No such persona.
    UnknownPersona {
        /// The persona id given.
        persona: String,
    },
    /// No such collection.
    UnknownCollection {
        /// The collection given.
        collection: String,
    },
    /// The user cancelled the platform prompt.
    Cancelled,
    /// The platform key root is unavailable, or did not seal what it was given.
    KeyRootUnavailable {
        /// The platform's message.
        detail: String,
    },
    /// Malformed: the sealed root blob's structure or format version, or the
    /// platform side found the sealed data key so.
    SealedRootMalformed {
        /// What.
        detail: String,
    },
    /// The sealed root did not open: another platform key, or tampered.
    UnsealFailed,
    /// The action was refused before it was queued; nothing was queued. With no data
    /// source connected, every action is refused this way. `[FR-C-4]` `[FR-C-5]`
    Refused {
        /// Why, for the screen ("no data source connected").
        reason: String,
    },
    /// A file could not be read.
    NotFound {
        /// What.
        what: String,
    },
    /// An internal error.
    Internal {
        /// What.
        detail: String,
    },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ClientError {}

impl From<CoreError> for ClientError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Stale { found, min } => ClientError::Stale { found, min },
            CoreError::PendingOps(n) => ClientError::PendingOps {
                count: u32::try_from(n).unwrap_or(u32::MAX),
            },
            CoreError::Reserved(what) => ClientError::Reserved { what },
            CoreError::Malformed(d) | CoreError::MalformedOp(d) => {
                ClientError::Malformed { detail: d }
            }
            CoreError::Store(s) => match s {
                StoreError::AboveClearance => ClientError::AboveClearance,
                StoreError::Stale { found, min } => ClientError::Stale { found, min },
                StoreError::Malformed(d) => ClientError::Malformed { detail: d },
                StoreError::Kms(k) => ClientError::Integrity {
                    detail: k.to_string(),
                },
                other => ClientError::Internal {
                    detail: other.to_string(),
                },
            },
        }
    }
}

impl From<SourceError> for ClientError {
    fn from(e: SourceError) -> Self {
        match e {
            SourceError::Offline => ClientError::Offline,
            SourceError::UnknownPersona(persona) => ClientError::UnknownPersona { persona },
            SourceError::UnknownCollection(collection) => {
                ClientError::UnknownCollection { collection }
            }
            SourceError::Malformed(detail) => ClientError::Malformed { detail },
            e @ SourceError::NoSource => ClientError::Refused {
                reason: e.to_string(),
            },
        }
    }
}

impl From<DeviceKeyError> for ClientError {
    fn from(e: DeviceKeyError) -> Self {
        ClientError::DeviceKey {
            detail: e.to_string(),
        }
    }
}

impl From<SessionError> for ClientError {
    fn from(e: SessionError) -> Self {
        match e {
            SessionError::AboveClearance => ClientError::AboveClearance,
            SessionError::Offline => ClientError::Offline,
            SessionError::NothingCached(collection) => ClientError::NothingCached { collection },
            SessionError::PendingOps(n) => ClientError::PendingOps {
                count: u32::try_from(n).unwrap_or(u32::MAX),
            },
            SessionError::BadTarget(target) => ClientError::BadTarget { target },
            SessionError::Device(d) => d.into(),
            SessionError::Source(s) => s.into(),
            SessionError::Core(c) => c.into(),
            SessionError::Decode(d) => ClientError::Malformed {
                detail: format!("{}: {}", d.path, d.reason),
            },
        }
    }
}

impl From<DatasetError> for ClientError {
    fn from(e: DatasetError) -> Self {
        match e {
            DatasetError::Body(detail) => ClientError::Malformed { detail },
            other => ClientError::SignatureFailure {
                detail: other.to_string(),
            },
        }
    }
}

impl From<KeystoreError> for ClientError {
    fn from(e: KeystoreError) -> Self {
        match e {
            KeystoreError::UserCancelled => ClientError::Cancelled,
            KeystoreError::PlatformUnavailable(detail) => {
                ClientError::KeyRootUnavailable { detail }
            }
            KeystoreError::Malformed(detail) => ClientError::SealedRootMalformed { detail },
            KeystoreError::Unseal => ClientError::UnsealFailed,
            other => ClientError::Internal {
                detail: other.to_string(),
            },
        }
    }
}
