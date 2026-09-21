//! The append-only, signed config history.

use pqc_agility::{CryptoProvider, SignatureScheme, SigningKey, VerifyingKey};

use crate::commit::{commit_preimage, Commit};
use crate::diff::ConfigDiff;
use crate::error::ConfigError;
use crate::state::ConfigState;

/// A versioned config store: an append-only list of PQ-signed commits.
pub struct ConfigStore {
    scheme: Box<dyn SignatureScheme>,
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    history: Vec<Commit>,
}

impl ConfigStore {
    /// Build an empty store over a commit signer.
    pub fn new(
        scheme: Box<dyn SignatureScheme>,
        signing_key: SigningKey,
        verifying_key: VerifyingKey,
    ) -> Self {
        Self {
            scheme,
            signing_key,
            verifying_key,
            history: Vec::new(),
        }
    }

    /// The verifying key for this store's commit signatures.
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// The full history, oldest first.
    pub fn history(&self) -> &[Commit] {
        &self.history
    }

    /// Rehydrate a previously-signed commit WITHOUT re-signing — the durable store
    /// uses this to reload persisted commits on boot; the chain is re-verified
    /// afterwards via [`ConfigStore::verify`]. [RC-14]
    pub fn load_commit(&mut self, commit: Commit) {
        self.history.push(commit);
    }

    /// The current head commit, if any.
    pub fn head(&self) -> Option<&Commit> {
        self.history.last()
    }

    /// The commit at a given version.
    pub fn get_version(&self, version: u64) -> Option<&Commit> {
        self.history.get(version as usize)
    }

    /// Commit a new config snapshot: content-hash it, link it to the current head,
    /// PQ-sign the metadata, and append it. Returns the new commit. `[FR-AD-7]`
    pub fn commit(
        &mut self,
        state: ConfigState,
        author: &str,
        message: &str,
    ) -> Result<&Commit, ConfigError> {
        if author.is_empty() {
            return Err(ConfigError::EmptyAuthor);
        }
        let version = self.history.len() as u64;
        let parent = self.history.last().map(|c| c.content_hash);
        let content_hash = state.content_hash();
        let preimage = commit_preimage(version, &parent, &content_hash, author, message);
        let signature = self
            .scheme
            .sign(&self.signing_key, &preimage)
            .map_err(|e| ConfigError::Sign(format!("{e:?}")))?;
        self.history.push(Commit {
            version,
            parent,
            content_hash,
            author: author.to_string(),
            message: message.to_string(),
            state,
            sig_alg: self.signing_key.alg(),
            signature,
        });
        Ok(self.history.last().expect("just pushed"))
    }

    /// Diff two versions (`from` → `to`). `[FR-AD-7]`
    pub fn diff(&self, from: u64, to: u64) -> Result<ConfigDiff, ConfigError> {
        let a = self
            .get_version(from)
            .ok_or(ConfigError::UnknownVersion(from))?;
        let b = self
            .get_version(to)
            .ok_or(ConfigError::UnknownVersion(to))?;
        Ok(ConfigDiff::between(&a.state, &b.state))
    }

    /// Roll back to an earlier version by committing a copy of its state forward.
    /// The history is never rewritten — rollback is an append-only operation, so
    /// the rollback itself is versioned and auditable. `[FR-AD-7]` `[AC-AD-7]`
    pub fn rollback(
        &mut self,
        to_version: u64,
        author: &str,
        message: &str,
    ) -> Result<&Commit, ConfigError> {
        let restored = self
            .get_version(to_version)
            .ok_or(ConfigError::UnknownVersion(to_version))?
            .state
            .clone();
        self.commit(restored, author, message)
    }

    /// Verify the whole history: every content hash matches its state, every parent
    /// links to the prior commit, and every commit signature verifies. Fails closed
    /// at the first defect, naming the version. `[NFR-PQ-3]`
    pub fn verify(&self, provider: &CryptoProvider) -> Result<(), ConfigError> {
        let mut expected_parent: Option<[u8; 32]> = None;
        for c in &self.history {
            if c.state.content_hash() != c.content_hash {
                return Err(ConfigError::ContentHashMismatch { version: c.version });
            }
            if c.parent != expected_parent {
                return Err(ConfigError::ChainBroken { version: c.version });
            }
            let scheme = provider
                .sig_for(c.sig_alg)
                .map_err(|e| ConfigError::Sign(format!("{e:?}")))?;
            let preimage =
                commit_preimage(c.version, &c.parent, &c.content_hash, &c.author, &c.message);
            scheme
                .verify(&self.verifying_key, &preimage, &c.signature)
                .map_err(|_| ConfigError::SignatureInvalid { version: c.version })?;
            expected_parent = Some(c.content_hash);
        }
        Ok(())
    }
}
