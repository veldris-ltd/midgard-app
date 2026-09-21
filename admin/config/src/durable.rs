//! Durable, KMS-sealed config history. Every signed [`Commit`] is appended to a
//! [`SealedJournal`] (encrypted + `fsync`ed at rest); on [`DurableConfigStore::open`]
//! the journal is decrypted and replayed into a [`ConfigStore`], whose signed chain
//! is then re-verified. So config history survives a restart and is re-verified on
//! boot. [FR-AD-7] [RC-14]

use audit_writer::{SealedJournal, Sealer};
use pqc_agility::{CryptoProvider, SigId, Signature, SignatureScheme, SigningKey, VerifyingKey};

use crate::commit::Commit;
use crate::error::ConfigError;
use crate::state::ConfigState;
use crate::store::ConfigStore;

const DOMAIN: &[u8] = b"midgard-admin-config-v1";

/// A config store whose signed commit history is durable and recovered on boot.
pub struct DurableConfigStore {
    inner: ConfigStore,
    journal: SealedJournal,
}

impl DurableConfigStore {
    /// Open (creating if absent) the durable store, replaying persisted commits.
    pub fn open(
        path: impl AsRef<std::path::Path>,
        sealer: Box<dyn Sealer>,
        scheme: Box<dyn SignatureScheme>,
        signing_key: SigningKey,
        verifying_key: VerifyingKey,
    ) -> Result<Self, ConfigError> {
        let (journal, records) = SealedJournal::open(path, sealer, DOMAIN)
            .map_err(|e| ConfigError::Io(e.to_string()))?;
        let mut inner = ConfigStore::new(scheme, signing_key, verifying_key);
        for r in &records {
            inner.load_commit(decode_commit(r)?);
        }
        Ok(Self { inner, journal })
    }

    /// Commit a config snapshot (signed) and durably persist it.
    pub fn commit(
        &mut self,
        state: ConfigState,
        author: &str,
        message: &str,
    ) -> Result<&Commit, ConfigError> {
        self.inner.commit(state, author, message)?;
        let committed = self.inner.head().expect("just committed").clone();
        self.journal
            .append(&encode_commit(&committed))
            .map_err(|e| ConfigError::Io(e.to_string()))?;
        Ok(self.inner.head().expect("head"))
    }

    /// Roll back to an earlier version (an append-only, durable, signed commit).
    pub fn rollback(
        &mut self,
        to_version: u64,
        author: &str,
        message: &str,
    ) -> Result<&Commit, ConfigError> {
        let restored = self
            .inner
            .get_version(to_version)
            .ok_or(ConfigError::UnknownVersion(to_version))?
            .state
            .clone();
        self.commit(restored, author, message)
    }

    /// Re-verify the whole signed history (used on boot). [NFR-PQ-3]
    pub fn verify(&self, provider: &CryptoProvider) -> Result<(), ConfigError> {
        self.inner.verify(provider)
    }

    /// The full history, oldest first.
    pub fn history(&self) -> &[Commit] {
        self.inner.history()
    }
    /// The head commit.
    pub fn head(&self) -> Option<&Commit> {
        self.inner.head()
    }
    /// The commit at a version.
    pub fn get_version(&self, version: u64) -> Option<&Commit> {
        self.inner.get_version(version)
    }
    /// The commit-signature verifying key.
    pub fn verifying_key(&self) -> &VerifyingKey {
        self.inner.verifying_key()
    }
}

// --- Commit codec ---------------------------------------------------------

fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

fn encode_commit(c: &Commit) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend_from_slice(&c.version.to_le_bytes());
    match &c.parent {
        Some(h) => {
            o.push(1);
            o.extend_from_slice(h);
        }
        None => o.push(0),
    }
    o.extend_from_slice(&c.content_hash);
    put_str(&mut o, &c.author);
    put_str(&mut o, &c.message);
    let entries = c.state.entries();
    o.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (k, v) in entries {
        put_str(&mut o, k);
        put_str(&mut o, v);
    }
    o.push(c.sig_alg.as_u8());
    o.extend_from_slice(&(c.signature.as_bytes().len() as u32).to_le_bytes());
    o.extend_from_slice(c.signature.as_bytes());
    o
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ConfigError> {
        let e = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| ConfigError::Io("truncated commit record".into()))?;
        let s = &self.b[self.p..e];
        self.p = e;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, ConfigError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<usize, ConfigError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn u64(&mut self) -> Result<u64, ConfigError> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    fn h32(&mut self) -> Result<[u8; 32], ConfigError> {
        let mut a = [0u8; 32];
        a.copy_from_slice(self.take(32)?);
        Ok(a)
    }
    fn s(&mut self) -> Result<String, ConfigError> {
        let n = self.u32()?;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| ConfigError::Io("commit record invalid utf8".into()))
    }
}

fn decode_commit(b: &[u8]) -> Result<Commit, ConfigError> {
    let mut r = R { b, p: 0 };
    let version = r.u64()?;
    let parent = match r.u8()? {
        0 => None,
        1 => Some(r.h32()?),
        _ => return Err(ConfigError::Io("bad parent tag".into())),
    };
    let content_hash = r.h32()?;
    let author = r.s()?;
    let message = r.s()?;
    let n = r.u32()?;
    let mut state = ConfigState::new();
    for _ in 0..n {
        let k = r.s()?;
        let v = r.s()?;
        state.set(k, v);
    }
    let sig_alg =
        SigId::from_u8(r.u8()?).ok_or_else(|| ConfigError::Io("unknown sig alg".into()))?;
    let siglen = r.u32()?;
    let signature = Signature::new(sig_alg, r.take(siglen)?.to_vec());
    if r.p != b.len() {
        return Err(ConfigError::Io("commit record trailing bytes".into()));
    }
    Ok(Commit {
        version,
        parent,
        content_hash,
        author,
        message,
        state,
        sig_alg,
        signature,
    })
}
