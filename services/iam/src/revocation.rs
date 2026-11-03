//! The deny list of revoked access tokens. A token is self-contained and verifies
//! offline, so ending a session before its expiry needs the verifier to remember
//! which token ids it must refuse. An entry only has to outlive the token it names:
//! once `expires_at` has passed the signature check refuses the token anyway, so
//! [`RevocationList::purge`] drops it and the list never holds more than the
//! sign-outs of one token lifetime. Durability is the caller's: the gateway journals
//! each revocation before recording it here. [NFR-SEC-2] [VLD-ANB-MIDGARD-0021]

use std::collections::BTreeMap;

/// Revoked token ids, each with the expiry of the token it names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevocationList {
    denied: BTreeMap<[u8; 16], u64>,
}

impl RevocationList {
    /// An empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Refuse `token_id` from now on, until `expires_at` (Unix seconds, exclusive),
    /// after which the token is expired and the entry is no longer needed.
    pub fn revoke(&mut self, token_id: [u8; 16], expires_at: u64) {
        let until = self.denied.entry(token_id).or_insert(expires_at);
        *until = (*until).max(expires_at);
    }

    /// Whether `token_id` has been revoked and not yet purged.
    pub fn is_revoked(&self, token_id: &[u8; 16]) -> bool {
        self.denied.contains_key(token_id)
    }

    /// Drop every entry whose token has expired at `now`: its expiry is at or before
    /// `now`, the same rule `verify_token` refuses an expired token by.
    pub fn purge(&mut self, now: u64) {
        self.denied.retain(|_, expires_at| *expires_at > now);
    }

    /// The number of entries held.
    pub fn len(&self) -> usize {
        self.denied.len()
    }

    /// Whether no entry is held.
    pub fn is_empty(&self) -> bool {
        self.denied.is_empty()
    }

    /// The entries held, in token-id order: what a durable store writes back.
    pub fn entries(&self) -> impl Iterator<Item = ([u8; 16], u64)> + '_ {
        self.denied.iter().map(|(id, until)| (*id, *until))
    }
}
