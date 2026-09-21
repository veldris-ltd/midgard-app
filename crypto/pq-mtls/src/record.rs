//! Post-handshake record protection: bulk AES-256-GCM over the session traffic
//! secret. The handshake ([`crate::handshake`]) establishes the shared secret;
//! this layer carries application bytes under it. [NFR-SEC-1] [NFR-PQ-2]
//!
//! Two independent directional keys are derived from the traffic secret so the
//! two directions never share a keystream. Each direction has its own monotonic
//! 64-bit sequence number that forms the GCM nonce, so:
//! - a replayed or reordered record decrypts under the wrong nonce and **fails
//!   the tag check** (fail closed);
//! - a (key, nonce) pair never repeats within a session.
//!
//! Requires the production AEAD, so this module is gated on `backend-fips`.

use pqc_agility::hash::hkdf_sha256_32;
use pqc_agility::{Aes256GcmAead, GCM_NONCE_LEN};

use crate::error::MtlsError;
use crate::session::Session;

const RECORD_INFO: &[u8] = b"midgard-pqmtls-record-v1";
const LABEL_C2S: &[u8] = b"client-to-server";
const LABEL_S2C: &[u8] = b"server-to-client";

/// Which end of the session a party is, which fixes its send/receive keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The connecting side (client): sends on c2s, receives on s2c.
    Initiator,
    /// The accepting side (server): sends on s2c, receives on c2s.
    Responder,
}

fn dir_key(traffic: &[u8; 32], label: &[u8]) -> [u8; 32] {
    hkdf_sha256_32(label, traffic, RECORD_INFO)
}

/// The GCM nonce for a record: 4 zero bytes then the big-endian sequence number.
fn nonce_for(seq: u64) -> [u8; GCM_NONCE_LEN] {
    let mut n = [0u8; GCM_NONCE_LEN];
    n[4..].copy_from_slice(&seq.to_be_bytes());
    n
}

/// The sending half of a session's record stream.
pub struct RecordSealer {
    aead: Aes256GcmAead,
    seq: u64,
}

impl RecordSealer {
    /// Seal one record, binding `aad`. Returns `ciphertext ‖ tag`. The sequence
    /// number advances; exhausting it (2^64 records) fails closed rather than
    /// reusing a nonce.
    pub fn seal(&mut self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, MtlsError> {
        let nonce = nonce_for(self.seq);
        let ct = self
            .aead
            .seal(&nonce, aad, plaintext)
            .map_err(MtlsError::Crypto)?;
        self.seq = self
            .seq
            .checked_add(1)
            .ok_or_else(|| MtlsError::Protocol("record sequence exhausted".into()))?;
        Ok(ct)
    }
}

/// The receiving half of a session's record stream.
pub struct RecordOpener {
    aead: Aes256GcmAead,
    seq: u64,
}

impl RecordOpener {
    /// Open the next record in sequence. A reordered, replayed, or tampered
    /// record fails the tag check and does not advance the stream.
    pub fn open(&mut self, aad: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, MtlsError> {
        let nonce = nonce_for(self.seq);
        let pt = self
            .aead
            .open(&nonce, aad, ciphertext)
            .map_err(MtlsError::Crypto)?;
        self.seq = self
            .seq
            .checked_add(1)
            .ok_or_else(|| MtlsError::Protocol("record sequence exhausted".into()))?;
        Ok(pt)
    }
}

impl Session {
    /// Derive this session's `(sealer, opener)` for the given role. Both ends of
    /// the same session produce complementary halves: what the initiator seals,
    /// the responder opens, and vice versa.
    pub fn records(&self, role: Role) -> (RecordSealer, RecordOpener) {
        let traffic = self.traffic_secret();
        let c2s = dir_key(traffic, LABEL_C2S);
        let s2c = dir_key(traffic, LABEL_S2C);
        let (send, recv) = match role {
            Role::Initiator => (c2s, s2c),
            Role::Responder => (s2c, c2s),
        };
        (
            RecordSealer {
                aead: Aes256GcmAead::new(&send),
                seq: 0,
            },
            RecordOpener {
                aead: Aes256GcmAead::new(&recv),
                seq: 0,
            },
        )
    }
}
