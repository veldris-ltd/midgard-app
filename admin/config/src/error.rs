//! Config-store errors. Fail closed. [FR-AD-7]

/// A failure committing, diffing, rolling back, or verifying config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The author field was empty.
    EmptyAuthor,
    /// A referenced version does not exist.
    UnknownVersion(u64),
    /// Signing the commit failed.
    Sign(String),
    /// A commit signature did not verify.
    SignatureInvalid {
        /// The offending version.
        version: u64,
    },
    /// The parent link at a version does not match the prior commit's content hash.
    ChainBroken {
        /// The offending version.
        version: u64,
    },
    /// A commit's stored content hash does not match its state.
    /// The durable store failed to persist/recover (I/O or a malformed record). [RC-14]
    Io(String),
    ContentHashMismatch {
        /// The offending version.
        version: u64,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::EmptyAuthor => write!(f, "commit author must not be empty"),
            ConfigError::UnknownVersion(v) => write!(f, "unknown config version {v}"),
            ConfigError::Sign(e) => write!(f, "commit signing failed: {e}"),
            ConfigError::SignatureInvalid { version } => {
                write!(f, "commit signature invalid at version {version}")
            }
            ConfigError::ChainBroken { version } => {
                write!(f, "parent linkage broken at version {version}")
            }
            ConfigError::Io(m) => write!(f, "durable config store i/o: {m}"),
            ConfigError::ContentHashMismatch { version } => {
                write!(f, "content hash mismatch at version {version}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}
