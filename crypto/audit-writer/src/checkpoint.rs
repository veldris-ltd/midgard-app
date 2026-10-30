//! The signed ledger checkpoint: the ledger's length and head, vouched for by the
//! audit key. [MG-SEC-05]
//!
//! A hash chain proves every entry it still holds, but not that nothing was cut off
//! its end: drop the last *k* entries and what remains is a perfectly valid, shorter
//! chain. The checkpoint closes that gap. After every append the [`AuditWriter`]
//! signs `(ledger id, entry count, head entry hash)` and the store keeps it beside
//! the ledger, so a ledger shorter than its checkpoint, or whose head is not the
//! checkpointed one, fails closed.
//!
//! The ledger may be **one** entry ahead of its checkpoint and no more: an entry is
//! `fsync`ed before its checkpoint is written, so a crash between the two leaves
//! exactly that, and the extra entry is still chained and signed like any other.
//!
//! What a checkpoint kept on the same disk cannot catch is a rollback to an *older*
//! checkpoint that was copied aside earlier, with the ledger cut back to match it.
//! Each checkpoint is a self-contained signed record ([`LedgerCheckpoint::encode`])
//! precisely so it can be copied off the server and compared later.
//!
//! [`AuditWriter`]: crate::ledger::AuditWriter

use pqc_agility::{CryptoProvider, SigId, Signature, SignatureScheme, SigningKey, VerifyingKey};

use crate::chain::{checkpoint_preimage, genesis_hash};
use crate::error::AuditError;
use crate::record::{LedgerId, SealedEntry};

const CHECKPOINT_SCHEMA: u8 = 1;
const CHECKPOINT_MAGIC: &[u8] = b"MGCP";

/// A signed statement of a ledger's length and head entry. [MG-SEC-05]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerCheckpoint {
    /// The ledger it vouches for.
    pub ledger_id: LedgerId,
    /// How many entries the ledger held.
    pub count: u64,
    /// The hash of entry `count - 1`, or the ledger's genesis hash when empty.
    pub head_hash: [u8; 32],
    /// Algorithm of `signature`.
    pub sig_alg: SigId,
    /// Signature over [`checkpoint_preimage`].
    pub signature: Signature,
}

/// The `(count, head hash)` a checkpoint of `entries` states.
fn head_of(entries: &[SealedEntry], ledger_id: &LedgerId) -> (u64, [u8; 32]) {
    match entries.last() {
        Some(e) => (entries.len() as u64, e.entry_hash),
        None => (0, genesis_hash(ledger_id)),
    }
}

impl LedgerCheckpoint {
    /// Sign a checkpoint of `entries` as they stand.
    pub fn sign(
        scheme: &dyn SignatureScheme,
        signing_key: &SigningKey,
        ledger_id: &LedgerId,
        entries: &[SealedEntry],
    ) -> Result<Self, AuditError> {
        let (count, head_hash) = head_of(entries, ledger_id);
        let signature = scheme.sign(
            signing_key,
            &checkpoint_preimage(ledger_id, count, &head_hash),
        )?;
        Ok(Self {
            ledger_id: ledger_id.clone(),
            count,
            head_hash,
            sig_alg: signing_key.alg(),
            signature,
        })
    }

    /// Verify the checkpoint's signature under `verifying_key` and that it names
    /// `ledger_id`. Fails closed. [MG-SEC-05]
    pub fn verify_signature(
        &self,
        provider: &CryptoProvider,
        ledger_id: &LedgerId,
        verifying_key: &VerifyingKey,
    ) -> Result<(), AuditError> {
        if &self.ledger_id != ledger_id {
            return Err(AuditError::CheckpointInvalid(format!(
                "it is for ledger {:?}, not {:?}",
                self.ledger_id.0, ledger_id.0
            )));
        }
        if self.sig_alg != verifying_key.alg() {
            return Err(AuditError::CheckpointInvalid(
                "it is signed under another algorithm than the audit key's".into(),
            ));
        }
        let scheme = provider
            .sig_for(self.sig_alg)
            .map_err(|_| AuditError::CheckpointInvalid("no backend for its algorithm".into()))?;
        scheme
            .verify(
                verifying_key,
                &checkpoint_preimage(&self.ledger_id, self.count, &self.head_hash),
                &self.signature,
            )
            .map_err(|_| AuditError::CheckpointInvalid("its signature does not verify".into()))
    }

    /// Check `entries` against the checkpoint without any key: the ledger must hold
    /// the checkpointed head at the checkpointed position, and be at most one entry
    /// ahead of it. Catches a cut-off tail on its own; a forged checkpoint needs
    /// [`verify_signature`](Self::verify_signature) as well. [MG-SEC-05]
    pub fn check_covers(&self, entries: &[SealedEntry]) -> Result<(), AuditError> {
        let found = entries.len() as u64;
        if found < self.count {
            return Err(AuditError::Truncated {
                checkpoint: self.count,
                found,
            });
        }
        if found > self.count.saturating_add(1) {
            return Err(AuditError::CheckpointStale {
                checkpoint: self.count,
                found,
            });
        }
        let head = match self.count {
            0 => genesis_hash(&self.ledger_id),
            n => entries[(n - 1) as usize].entry_hash,
        };
        if head != self.head_hash {
            return Err(AuditError::CheckpointMismatch {
                seq: self.count.saturating_sub(1),
            });
        }
        Ok(())
    }

    /// Encode to a self-describing byte form (magic, schema, then length-prefixed
    /// fields), suitable for keeping beside the ledger or copying off the server.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.extend_from_slice(CHECKPOINT_MAGIC);
        o.push(CHECKPOINT_SCHEMA);
        o.extend_from_slice(&(self.ledger_id.0.len() as u32).to_le_bytes());
        o.extend_from_slice(self.ledger_id.0.as_bytes());
        o.extend_from_slice(&self.count.to_le_bytes());
        o.extend_from_slice(&self.head_hash);
        o.push(self.sig_alg.as_u8());
        let sig = self.signature.as_bytes();
        o.extend_from_slice(&(sig.len() as u32).to_le_bytes());
        o.extend_from_slice(sig);
        o
    }

    /// Decode [`encode`](Self::encode)'s form. Fails closed on a wrong magic or
    /// schema, truncation, trailing bytes, or an unknown algorithm.
    pub fn decode(b: &[u8]) -> Result<Self, AuditError> {
        fn bad(why: &str) -> AuditError {
            AuditError::CheckpointInvalid(why.to_string())
        }
        struct Reader<'a> {
            b: &'a [u8],
            pos: usize,
        }
        impl<'a> Reader<'a> {
            fn take(&mut self, n: usize) -> Result<&'a [u8], AuditError> {
                let end = self
                    .pos
                    .checked_add(n)
                    .filter(|&e| e <= self.b.len())
                    .ok_or_else(|| bad("truncated"))?;
                let s = &self.b[self.pos..end];
                self.pos = end;
                Ok(s)
            }
            fn len32(&mut self) -> Result<usize, AuditError> {
                let s = self.take(4)?;
                Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
            }
        }
        let mut r = Reader { b, pos: 0 };
        if r.take(CHECKPOINT_MAGIC.len())? != CHECKPOINT_MAGIC {
            return Err(bad("not a ledger checkpoint"));
        }
        if r.take(1)?[0] != CHECKPOINT_SCHEMA {
            return Err(bad("unknown checkpoint schema"));
        }
        let id_len = r.len32()?;
        let ledger_id =
            String::from_utf8(r.take(id_len)?.to_vec()).map_err(|_| bad("invalid utf8"))?;
        let mut count = [0u8; 8];
        count.copy_from_slice(r.take(8)?);
        let mut head_hash = [0u8; 32];
        head_hash.copy_from_slice(r.take(32)?);
        let sig_alg = SigId::from_u8(r.take(1)?[0]).ok_or_else(|| bad("unknown sig algorithm"))?;
        let sig_len = r.len32()?;
        let sig = r.take(sig_len)?.to_vec();
        if r.pos != b.len() {
            return Err(bad("trailing bytes"));
        }
        Ok(Self {
            ledger_id: LedgerId(ledger_id),
            count: u64::from_le_bytes(count),
            head_hash,
            sig_alg,
            signature: Signature::new(sig_alg, sig),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rejects_corruption() {
        let cp = LedgerCheckpoint {
            ledger_id: LedgerId("L".into()),
            count: 3,
            head_hash: [7u8; 32],
            sig_alg: SigId::MlDsa65,
            signature: Signature::new(SigId::MlDsa65, vec![1, 2, 3]),
        };
        let bytes = cp.encode();
        assert_eq!(LedgerCheckpoint::decode(&bytes).unwrap(), cp);
        assert!(LedgerCheckpoint::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(LedgerCheckpoint::decode(&extra).is_err());
        let mut magic = bytes;
        magic[0] ^= 1;
        assert!(LedgerCheckpoint::decode(&magic).is_err());
    }
}
