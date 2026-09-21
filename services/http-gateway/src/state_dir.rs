//! The on-disk layout of an enclosure's state directory — shared by `mg-admin`, which
//! creates it and enrols principals, and the gateway, which serves from it — and the
//! lock that keeps the two from writing the same journals at once. [RC-09] [RC-10]
//! [RC-29]
//!
//! ```text
//! <state-dir>/
//!   keys/idp.keystore      ML-DSA-65 issuer key; its custody root seals the directory
//!   keys/audit.keystore    ML-DSA-65 audit key; its custody root seals ledger and store
//!   trust/idp.pub          issuer verifying key, `<alg> <key-id> <hex>`
//!   trust/audit.pub        audit verifying key, same format
//!   policy.json            the discretionary rules the PDP enforces
//!   ontology.json          object types, link types and actions (registered each boot)
//!   identity.directory     enrolled principals (sealed, append-only)
//!   governance.ledger      the PQ-signed audit ledger (sealed, append-only)
//!   objects.store          the object fabric's objects and links (sealed, append-only)
//!   admin/passkeys.journal administrators' public passkeys (sealed, append-only)
//!   admin/roles.journal    administrators' role bindings (sealed, append-only)
//!   admin/privilege/       the dual-control service's ledger and in-flight approvals
//!   admin/pending.journal  enrolments waiting for a second administrator (sealed)
//!   midgard.lock           present while a gateway or mg-admin holds the directory
//! ```
//!
//! The keystores are the custody devices (see `attest::keystore`): whoever can read
//! `keys/` can sign as the enclosure and read its data, so protect that directory as a
//! secret. `trust/` holds only public keys.

use std::path::{Path, PathBuf};

use service_runtime::DirLock;

/// The lock file's name inside the state directory.
const LOCK_FILE: &str = "midgard.lock";

/// Key id of the IdP issuer key.
pub const IDP_KEY_ID: &str = "midgard-idp";
/// Key id of the governance audit key.
pub const AUDIT_KEY_ID: &str = "midgard-audit";
/// Id of the governance audit ledger.
pub const LEDGER_ID: &str = "midgard-governance";
/// Id of the object fabric's store.
pub const STORE_ID: &str = "midgard-objects";
/// Id of the dual-control service's audit ledger.
pub const PRIVILEGE_LEDGER_ID: &str = "midgard-admin-privilege";

/// Paths inside one enclosure's state directory.
#[derive(Debug, Clone)]
pub struct StateDir {
    root: PathBuf,
}

impl StateDir {
    /// The state directory rooted at `root`. Nothing is created or read.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the keystores live.
    pub fn keys(&self) -> PathBuf {
        self.root.join("keys")
    }

    /// Where the exported verifying keys live.
    pub fn trust(&self) -> PathBuf {
        self.root.join("trust")
    }

    /// The IdP issuer keystore.
    pub fn idp_keystore(&self) -> PathBuf {
        self.keys().join("idp.keystore")
    }

    /// The governance audit keystore.
    pub fn audit_keystore(&self) -> PathBuf {
        self.keys().join("audit.keystore")
    }

    /// The issuer verifying key, as a trust-anchor line.
    pub fn idp_trust_anchor(&self) -> PathBuf {
        self.trust().join("idp.pub")
    }

    /// The audit verifying key, as a trust-anchor line.
    pub fn audit_trust_anchor(&self) -> PathBuf {
        self.trust().join("audit.pub")
    }

    /// The policy file.
    pub fn policy(&self) -> PathBuf {
        self.root.join("policy.json")
    }

    /// The ontology file.
    pub fn ontology(&self) -> PathBuf {
        self.root.join("ontology.json")
    }

    /// The durable identity directory.
    pub fn directory(&self) -> PathBuf {
        self.root.join("identity.directory")
    }

    /// The durable audit ledger.
    pub fn ledger(&self) -> PathBuf {
        self.root.join("governance.ledger")
    }

    /// The durable object store.
    pub fn store(&self) -> PathBuf {
        self.root.join("objects.store")
    }

    /// Where administration state lives.
    pub fn admin(&self) -> PathBuf {
        self.root.join("admin")
    }

    /// Administrators' public passkeys.
    pub fn admin_passkeys(&self) -> PathBuf {
        self.admin().join("passkeys.journal")
    }

    /// Administrators' role bindings.
    pub fn admin_roles(&self) -> PathBuf {
        self.admin().join("roles.journal")
    }

    /// The dual-control service's ledger and in-flight approvals.
    pub fn admin_privilege(&self) -> PathBuf {
        self.admin().join("privilege")
    }

    /// Enrolments waiting for a second administrator.
    pub fn admin_pending(&self) -> PathBuf {
        self.admin().join("pending.journal")
    }

    /// The lock file.
    pub fn lock(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }
}

/// Exclusive use of a state directory, released when dropped — a [`DirLock`] on
/// `midgard.lock`, which takes over a lock whose holder has died.
#[derive(Debug)]
pub struct StateLock {
    _lock: DirLock,
}

impl StateLock {
    /// Take the lock, or explain who holds it.
    pub fn acquire(state: &StateDir, holder: &str) -> Result<Self, String> {
        DirLock::acquire(state.root(), LOCK_FILE, holder).map(|lock| Self { _lock: lock })
    }
}
