//! Single-use, time-boxed commit authorisation tokens. When an action clears
//! dual-control it is granted a [`CommitToken`] — a random secret bound to that one
//! action, valid only until `expires_at`, and burnt on first use. The commit step
//! demands a live token, so an authorisation cannot be replayed, cannot outlive its
//! window, and cannot be redirected to a different action. [FR-AD-5] [RC-31]

use std::collections::BTreeMap;

use pqc_agility::{OsRng, Rng};

use crate::error::OrchestratorError;

/// A single-use authorisation to commit one action. The `secret` is presented at
/// commit time; the orchestrator holds the matching record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitToken {
    /// The opaque token secret (hex of 32 random bytes).
    pub secret: String,
    /// The action this token authorises — a token is not transferable to another.
    pub action_id: String,
    /// When the token was issued (Unix seconds).
    pub issued_at: u64,
    /// When the token expires (issued_at + ttl).
    pub expires_at: u64,
}

struct TokenRecord {
    action_id: String,
    expires_at: u64,
    used: bool,
}

/// The orchestrator's token ledger: issues and single-use-consumes commit tokens.
#[derive(Default)]
pub struct TokenStore {
    records: BTreeMap<String, TokenRecord>,
}

impl TokenStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a fresh token for `action_id`, valid for `ttl` seconds from `now`.
    pub fn issue(&mut self, action_id: &str, now: u64, ttl: u64) -> CommitToken {
        let mut bytes = [0u8; 32];
        OsRng.fill(&mut bytes);
        let secret = hex(&bytes);
        let expires_at = now.saturating_add(ttl);
        self.records.insert(
            secret.clone(),
            TokenRecord {
                action_id: action_id.to_string(),
                expires_at,
                used: false,
            },
        );
        CommitToken {
            secret,
            action_id: action_id.to_string(),
            issued_at: now,
            expires_at,
        }
    }

    /// Consume a token for `action_id` at `now`. Fails closed on an unknown/mismatched
    /// token, an expired token, or a reused token; on success the token is burnt.
    pub fn consume(
        &mut self,
        secret: &str,
        action_id: &str,
        now: u64,
    ) -> Result<(), OrchestratorError> {
        let rec = self
            .records
            .get_mut(secret)
            .ok_or(OrchestratorError::InvalidToken)?;
        if rec.used {
            return Err(OrchestratorError::TokenReused);
        }
        if now >= rec.expires_at {
            return Err(OrchestratorError::TokenExpired);
        }
        if rec.action_id != action_id {
            return Err(OrchestratorError::InvalidToken);
        }
        rec.used = true;
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0x0f) as u32, 16).unwrap());
    }
    s
}
