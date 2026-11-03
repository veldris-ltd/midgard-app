//! Revoked session tokens, as the gateway keeps them: in memory for a development
//! gateway, or journaled and sealed in the state directory for a deployment. The
//! issuer key persists across restarts, so a token signed before one still verifies
//! after it; its revocation has to persist too, or a restart would bring a signed-out
//! token back. [NFR-SEC-2] [AC-M-4] [VLD-ANB-MIDGARD-0021]
//!
//! A revocation is written to the journal before it is recorded in memory, so one the
//! journal could not record never reports success. On open every record is replayed
//! and those whose token has already expired are purged, so memory holds no more than
//! the sign-outs of one token lifetime. The journal itself is append-only.

use std::path::Path;

use audit_writer::{AuditError, SealedJournal, Sealer};
use iam::RevocationList;

/// Namespaces the journal's records, so they can never be replayed into another store.
const DOMAIN: &[u8] = b"midgard-token-revocations-v1";

/// A record: the token id, then the token's expiry as a little-endian `u64`.
const RECORD_LEN: usize = 16 + 8;

/// Where revoked token ids are kept.
pub enum Revocations {
    /// Lost on restart, like everything else a development gateway holds.
    InMemory(RevocationList),
    /// Replayed from a sealed, append-only journal on open.
    Journaled {
        /// The live entries.
        list: RevocationList,
        /// Every revocation recorded, in order.
        journal: SealedJournal,
    },
}

impl Default for Revocations {
    fn default() -> Self {
        Revocations::InMemory(RevocationList::new())
    }
}

impl Revocations {
    /// Open (creating if absent) the journal at `path`, sealed by `sealer`, replaying
    /// every revocation and dropping those whose token has expired at `now`. Fails
    /// closed on a record that does not authenticate or decode.
    pub fn open(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        now: u64,
    ) -> Result<Self, AuditError> {
        let (journal, records) = SealedJournal::open(path, sealer, DOMAIN)?;
        let mut list = RevocationList::new();
        for record in records {
            let (token_id, expires_at) = decode(&record)?;
            list.revoke(token_id, expires_at);
        }
        list.purge(now);
        Ok(Revocations::Journaled { list, journal })
    }

    /// Refuse `token_id` until `expires_at`. A journaled list appends the record
    /// first, so an error leaves the token valid and the caller must not report it
    /// signed out.
    pub fn revoke(&mut self, token_id: [u8; 16], expires_at: u64) -> Result<(), AuditError> {
        match self {
            Revocations::InMemory(list) => list.revoke(token_id, expires_at),
            Revocations::Journaled { list, journal } => {
                journal.append(&encode(token_id, expires_at))?;
                list.revoke(token_id, expires_at);
            }
        }
        Ok(())
    }

    /// Purge what has expired at `now`, then say whether `token_id` is revoked.
    pub fn is_revoked(&mut self, token_id: &[u8; 16], now: u64) -> bool {
        let list = self.list_mut();
        list.purge(now);
        list.is_revoked(token_id)
    }

    /// The live entries held in memory.
    pub fn len(&self) -> usize {
        match self {
            Revocations::InMemory(list) | Revocations::Journaled { list, .. } => list.len(),
        }
    }

    /// Whether no live entry is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Records in the journal, purged or not; `None` for an in-memory list.
    pub fn journal_len(&self) -> Option<u64> {
        match self {
            Revocations::InMemory(_) => None,
            Revocations::Journaled { journal, .. } => Some(journal.len()),
        }
    }

    fn list_mut(&mut self) -> &mut RevocationList {
        match self {
            Revocations::InMemory(list) | Revocations::Journaled { list, .. } => list,
        }
    }
}

fn encode(token_id: [u8; 16], expires_at: u64) -> Vec<u8> {
    let mut record = Vec::with_capacity(RECORD_LEN);
    record.extend_from_slice(&token_id);
    record.extend_from_slice(&expires_at.to_le_bytes());
    record
}

fn decode(record: &[u8]) -> Result<([u8; 16], u64), AuditError> {
    if record.len() != RECORD_LEN {
        return Err(AuditError::Store(format!(
            "revocation record is {} bytes, not {RECORD_LEN}",
            record.len()
        )));
    }
    let mut token_id = [0u8; 16];
    token_id.copy_from_slice(&record[..16]);
    let mut expiry = [0u8; 8];
    expiry.copy_from_slice(&record[16..]);
    Ok((token_id, u64::from_le_bytes(expiry)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sealer that only binds the AAD, enough to exercise the journal's framing.
    struct PlainSealer;
    impl Sealer for PlainSealer {
        fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
            Ok([aad, plaintext].concat())
        }
        fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
            sealed
                .strip_prefix(aad)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| AuditError::Store("aad mismatch".into()))
        }
    }

    // [AC-M-4] [VLD-ANB-MIDGARD-0021]
    #[test]
    fn expired_revocations_are_purged_on_open_and_live_ones_replayed() {
        let path = std::env::temp_dir().join(format!(
            "revocations-unit-{}-{}.journal",
            std::process::id(),
            Box::into_raw(Box::new(0u8)) as usize
        ));
        {
            let mut revocations = Revocations::open(&path, Box::new(PlainSealer), 0).unwrap();
            revocations.revoke([1; 16], 100).unwrap();
            revocations.revoke([2; 16], 200).unwrap();
            assert_eq!(revocations.len(), 2);
        }
        let mut reopened = Revocations::open(&path, Box::new(PlainSealer), 150).unwrap();
        assert_eq!(reopened.len(), 1, "the expired record is not replayed");
        assert_eq!(
            reopened.journal_len(),
            Some(2),
            "the journal is append-only"
        );
        assert!(reopened.is_revoked(&[2; 16], 150));
        assert!(!reopened.is_revoked(&[1; 16], 150));
        let _ = std::fs::remove_file(&path);
    }

    // [VLD-ANB-MIDGARD-0021]
    #[test]
    fn a_record_of_the_wrong_length_fails_closed() {
        assert!(decode(&[0u8; RECORD_LEN - 1]).is_err());
        assert_eq!(decode(&encode([7; 16], 42)).unwrap(), ([7; 16], 42));
    }
}
