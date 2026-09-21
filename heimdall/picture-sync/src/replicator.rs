//! The replicator: export a picture as a PQ-signed delta, and apply a peer's delta
//! only after verifying its source signature. [FR-H-6] [NFR-PQ-3]

use pqc_agility::{SharedProvider, Signature, SigningKey, VerifyingKey};

use heimdall_picture::{Picture, PictureDelta};

use crate::error::SyncError;

/// A picture delta plus the source node's detached PQ signature over its canonical
/// bytes. The signature is what authenticates the delta's origin — the property a
/// receiver checks before merging.
#[derive(Debug, Clone, PartialEq)]
pub struct SignedDelta {
    /// Canonical delta bytes (`PictureDelta::canonical_bytes`).
    pub bytes: Vec<u8>,
    /// ML-DSA / SLH-DSA signature over `bytes`.
    pub signature: Signature,
}

/// Exports and imports signed picture deltas, resolving the signature scheme from
/// the shared agility provider so replication algorithms can rotate with no code
/// change.
pub struct Replicator {
    provider: SharedProvider,
}

impl Replicator {
    /// Build a replicator over the shared crypto provider.
    pub fn new(provider: SharedProvider) -> Self {
        Self { provider }
    }

    /// Export `picture` as a signed snapshot delta, signed with `signing_key`.
    pub fn export(
        &self,
        signing_key: &SigningKey,
        picture: &Picture,
    ) -> Result<SignedDelta, SyncError> {
        let bytes = PictureDelta::snapshot(picture).canonical_bytes();
        let provider = self.provider.read().expect("crypto provider lock poisoned");
        let scheme = provider.sig_for(signing_key.alg())?;
        let signature = scheme.sign(signing_key, &bytes)?;
        Ok(SignedDelta { bytes, signature })
    }

    /// Verify a peer's signed delta against `peer_key` and, if valid, merge it into
    /// `picture` (last-write-wins by `last_seen`), returning how many entities were
    /// inserted or updated. Fails closed — an unverified or malformed delta mutates
    /// nothing. [FR-H-6] [NFR-PQ-3] [AC-H-5]
    pub fn import(
        &self,
        peer_key: &VerifyingKey,
        delta: &SignedDelta,
        picture: &mut Picture,
    ) -> Result<usize, SyncError> {
        if delta.signature.alg() != peer_key.alg() {
            return Err(SyncError::AlgorithmMismatch {
                expected: peer_key.alg(),
                got: delta.signature.alg(),
            });
        }
        {
            let provider = self.provider.read().expect("crypto provider lock poisoned");
            let scheme = provider.sig_for(peer_key.alg())?;
            scheme
                .verify(peer_key, &delta.bytes, &delta.signature)
                .map_err(|_| SyncError::SignatureInvalid)?;
        }
        // Only now — after the signature checks out — decode and merge.
        let decoded = PictureDelta::from_canonical_bytes(&delta.bytes)?;
        Ok(decoded.merge_into(picture))
    }
}
