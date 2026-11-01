//! Signed, versioned certificate revocation lists, and live reloading. [RC-35]
//! [MG-SEC-03]
//!
//! A [`RevocationList`] is signed by the mesh CA and carries a version that only ever
//! goes up. A verifier installs it into its [`TrustAnchor`] with
//! [`TrustAnchor::install_crl`], which refuses a list the CA did not sign and a list
//! older than the one already installed. Because a trust anchor's revocation state is
//! shared by all its clones, [`spawn_crl_reloader`] can poll the list file and a newer
//! list takes effect on the next handshake or resumption, without a restart.
//!
//! `mg-admin mesh revoke` writes the list; services read it from `mesh.crl` beside
//! their certificate.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use pqc_agility::{CryptoError, CryptoProvider, SigId, Signature, SigningKey};

use crate::error::{MtlsError, TransportError};
use crate::identity::TrustAnchor;

/// File magic of a stored [`RevocationList`].
pub const CRL_MAGIC: &[u8; 6] = b"MGCRL1";

/// Domain separation for the signed list body.
const CRL_BODY_TAG: &[u8] = b"midgard-pqmtls-crl-v1";

/// A CA-signed list of revoked certificate fingerprints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevocationList {
    /// Monotonic version; a verifier never goes back to a lower one.
    pub version: u64,
    /// When the CA signed it (Unix seconds).
    pub issued_at: u64,
    /// Revoked verifying-key fingerprints (see [`crate::cert_fingerprint`]).
    pub revoked: BTreeSet<[u8; 32]>,
    /// Algorithm of `signature`.
    pub sig_alg: SigId,
    /// CA signature over [`canonical_crl_body`].
    pub signature: Signature,
}

/// The bytes the CA signs: a tag, the version, the issue time and the sorted
/// fingerprints.
pub fn canonical_crl_body(version: u64, issued_at: u64, revoked: &BTreeSet<[u8; 32]>) -> Vec<u8> {
    let mut o = CRL_BODY_TAG.to_vec();
    o.extend_from_slice(&version.to_le_bytes());
    o.extend_from_slice(&issued_at.to_le_bytes());
    o.extend_from_slice(&(revoked.len() as u32).to_le_bytes());
    for fp in revoked {
        o.extend_from_slice(fp);
    }
    o
}

impl RevocationList {
    /// Sign a list with the CA key.
    pub fn sign(
        provider: &CryptoProvider,
        ca_alg: SigId,
        ca_key: &SigningKey,
        version: u64,
        issued_at: u64,
        revoked: BTreeSet<[u8; 32]>,
    ) -> Result<Self, CryptoError> {
        let signature = provider
            .sig_for(ca_alg)?
            .sign(ca_key, &canonical_crl_body(version, issued_at, &revoked))?;
        Ok(Self {
            version,
            issued_at,
            revoked,
            sig_alg: ca_alg,
            signature,
        })
    }

    /// Check the CA signature under `anchor`. Fails closed.
    pub fn verify(&self, anchor: &TrustAnchor, provider: &CryptoProvider) -> Result<(), MtlsError> {
        if self.sig_alg != anchor.ca_alg {
            return Err(MtlsError::CrlUntrusted);
        }
        provider
            .sig_for(self.sig_alg)
            .map_err(|_| MtlsError::CrlUntrusted)?
            .verify(
                &anchor.ca_key,
                &canonical_crl_body(self.version, self.issued_at, &self.revoked),
                &self.signature,
            )
            .map_err(|_| MtlsError::CrlUntrusted)
    }
}

/// Encode a list for storage.
pub fn encode_crl(crl: &RevocationList) -> Vec<u8> {
    let mut o = CRL_MAGIC.to_vec();
    o.extend_from_slice(&crl.version.to_le_bytes());
    o.extend_from_slice(&crl.issued_at.to_le_bytes());
    o.extend_from_slice(&(crl.revoked.len() as u32).to_le_bytes());
    for fp in &crl.revoked {
        o.extend_from_slice(fp);
    }
    o.push(crl.sig_alg.as_u8());
    o.push(crl.signature.alg().as_u8());
    o.extend_from_slice(&(crl.signature.as_bytes().len() as u32).to_le_bytes());
    o.extend_from_slice(crl.signature.as_bytes());
    o
}

/// Decode a stored list. Structure only: the signature is checked when it is
/// installed. A wrong magic, truncated or trailing input is refused.
pub fn decode_crl(bytes: &[u8]) -> Result<RevocationList, TransportError> {
    let malformed = |s: &str| TransportError::Malformed(s.into());
    let mut b = bytes
        .strip_prefix(CRL_MAGIC.as_slice())
        .ok_or_else(|| malformed("not a revocation list file"))?;
    let mut take = |n: usize| -> Result<&[u8], TransportError> {
        if b.len() < n {
            return Err(malformed("revocation list truncated"));
        }
        let (head, rest) = b.split_at(n);
        b = rest;
        Ok(head)
    };
    let u64_at = |s: &[u8]| u64::from_le_bytes(s.try_into().expect("8 bytes"));
    let version = u64_at(take(8)?);
    let issued_at = u64_at(take(8)?);
    let n = u32::from_le_bytes(take(4)?.try_into().expect("4 bytes")) as usize;
    let mut revoked = BTreeSet::new();
    for _ in 0..n {
        revoked.insert(<[u8; 32]>::try_from(take(32)?).expect("32 bytes"));
    }
    let sig_alg = SigId::from_u8(take(1)?[0]).ok_or_else(|| malformed("unknown sig id"))?;
    let inner_alg = SigId::from_u8(take(1)?[0]).ok_or_else(|| malformed("unknown sig id"))?;
    if inner_alg != sig_alg {
        return Err(malformed("revocation list signature algorithm mismatch"));
    }
    let len = u32::from_le_bytes(take(4)?.try_into().expect("4 bytes")) as usize;
    let sig = take(len)?.to_vec();
    if !b.is_empty() {
        return Err(malformed("trailing bytes after revocation list"));
    }
    Ok(RevocationList {
        version,
        issued_at,
        revoked,
        sig_alg,
        signature: Signature::new(sig_alg, sig),
    })
}

/// Read the list at `path` and install it into `anchor`. Returns whether the anchor's
/// state changed. A missing, unreadable, unsigned, forged or older list is an error
/// and leaves the installed state as it was.
pub fn reload_crl_file(
    anchor: &TrustAnchor,
    path: &Path,
    provider: &CryptoProvider,
) -> Result<bool, MtlsError> {
    let bytes = std::fs::read(path).map_err(|e| {
        MtlsError::Transport(TransportError::Io(format!(
            "cannot read {}: {e}",
            path.display()
        )))
    })?;
    let crl = decode_crl(&bytes).map_err(MtlsError::Transport)?;
    anchor.install_crl(&crl, provider)
}

/// Poll `path` every `interval` and install any newer list into `anchor` (and so into
/// every clone of it). `on_event` hears each outcome that is not "unchanged": a newer
/// list installed (`Ok(true)`) or a refusal (`Err`), which a service logs. The thread
/// runs for the life of the process.
pub fn spawn_crl_reloader(
    anchor: TrustAnchor,
    path: PathBuf,
    interval: Duration,
    provider: CryptoProvider,
    on_event: impl Fn(Result<bool, MtlsError>) + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut last_mtime = None;
        loop {
            std::thread::sleep(interval);
            let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            if mtime.is_some() && mtime == last_mtime {
                continue;
            }
            last_mtime = mtime;
            match reload_crl_file(&anchor, &path, &provider) {
                Ok(false) => {}
                outcome => on_event(outcome),
            }
        }
    })
}
