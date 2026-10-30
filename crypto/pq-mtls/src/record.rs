//! Post-handshake record protection: bulk AES-256-GCM over the session traffic
//! secret. The handshake ([`crate::handshake`]) establishes the shared secret;
//! this layer carries application bytes under it. [NFR-SEC-1] [NFR-PQ-2]
//!
//! Two independent directional keys are derived from the traffic secret so the
//! two directions never share a keystream. Each direction has its own monotonic
//! sequence number that forms the GCM nonce, so:
//! - a replayed or reordered record decrypts under the wrong nonce and **fails
//!   the tag check** (fail closed);
//! - a (key, nonce) pair never repeats within a session.
//!
//! Protocol version 2 adds two things [MG-SEC-12]:
//! - **Rekeying.** After [`RekeyLimits::records`] records or [`RekeyLimits::bytes`]
//!   bytes under one key — far inside AES-GCM's safe usage bounds, where the old limit
//!   was 2^64 records — each direction ratchets its key forward through HKDF and
//!   restarts its sequence. Both ends count the same records and bytes, so they
//!   ratchet at the same point with no message exchanged, and an old key cannot be
//!   recovered from a new one.
//! - **An explicit end of stream.** Every record's plaintext starts with a content
//!   type; [`RecordSealer::seal_close`] sends the end-of-stream marker. A reader that
//!   reaches the end of the connection without it was cut off, not finished, and
//!   [`MtlsError::StreamTruncated`] says so.
//!
//! Requires the production AEAD, so this module is gated on `backend-fips`.

use pqc_agility::hash::hkdf_sha256_32;
use pqc_agility::{Aes256GcmAead, SecretArray, GCM_NONCE_LEN};

use crate::error::MtlsError;
use crate::session::Session;

const RECORD_INFO: &[u8] = b"midgard-pqmtls-record-v2";
const REKEY_INFO: &[u8] = b"midgard-pqmtls-rekey-v2";
const LABEL_C2S: &[u8] = b"client-to-server";
const LABEL_S2C: &[u8] = b"server-to-client";

/// Content type of an application-data record.
const TYPE_DATA: u8 = 0x00;
/// Content type of the end-of-stream marker.
const TYPE_CLOSE: u8 = 0x01;

/// Which end of the session a party is, which fixes its send/receive keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The connecting side (client): sends on c2s, receives on s2c.
    Initiator,
    /// The accepting side (server): sends on s2c, receives on c2s.
    Responder,
}

/// How much traffic one key protects before both ends ratchet to the next. [MG-SEC-12]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RekeyLimits {
    /// Records per key.
    pub records: u64,
    /// Plaintext bytes per key.
    pub bytes: u64,
}

impl Default for RekeyLimits {
    /// 2^24 records or 2^34 bytes (16 GiB), whichever comes first: in line with the
    /// TLS 1.3 guidance for AES-GCM (RFC 8446 §5.5), with margin.
    fn default() -> Self {
        Self {
            records: 1 << 24,
            bytes: 1 << 34,
        }
    }
}

/// One record as read: application data, or the peer's end-of-stream marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// Application data.
    Data(Vec<u8>),
    /// The peer finished the stream.
    Close,
}

fn dir_key(traffic: &[u8; 32], label: &[u8]) -> SecretArray<32> {
    SecretArray::new(hkdf_sha256_32(label, traffic, RECORD_INFO))
}

/// The GCM nonce for a record: 4 zero bytes then the big-endian sequence number.
fn nonce_for(seq: u64) -> [u8; GCM_NONCE_LEN] {
    let mut n = [0u8; GCM_NONCE_LEN];
    n[4..].copy_from_slice(&seq.to_be_bytes());
    n
}

/// One direction's key, sequence and usage counters.
struct Direction {
    key: SecretArray<32>,
    aead: Aes256GcmAead,
    seq: u64,
    bytes: u64,
    epoch: u64,
    limits: RekeyLimits,
}

impl Direction {
    fn new(key: SecretArray<32>, limits: RekeyLimits) -> Self {
        let aead = Aes256GcmAead::new(key.expose());
        Self {
            key,
            aead,
            seq: 0,
            bytes: 0,
            epoch: 0,
            limits,
        }
    }

    /// Count one record of `len` plaintext bytes, and ratchet the key once the limits
    /// are reached. Fails closed if the epoch counter would wrap.
    fn advance(&mut self, len: usize) -> Result<(), MtlsError> {
        self.seq += 1;
        self.bytes = self.bytes.saturating_add(len as u64);
        if self.seq >= self.limits.records || self.bytes >= self.limits.bytes {
            self.epoch = self
                .epoch
                .checked_add(1)
                .ok_or_else(|| MtlsError::Protocol("record key epochs exhausted".into()))?;
            let next = SecretArray::new(hkdf_sha256_32(
                &self.epoch.to_be_bytes(),
                self.key.expose(),
                REKEY_INFO,
            ));
            self.aead = Aes256GcmAead::new(next.expose());
            self.key = next;
            self.seq = 0;
            self.bytes = 0;
        }
        Ok(())
    }
}

/// The sending half of a session's record stream.
pub struct RecordSealer {
    dir: Direction,
    closed: bool,
}

impl RecordSealer {
    /// Seal one data record, binding `aad`. Returns `ciphertext ‖ tag`.
    pub fn seal(&mut self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, MtlsError> {
        self.seal_typed(TYPE_DATA, aad, plaintext)
    }

    /// Seal the end-of-stream marker. Nothing can be sealed after it. [MG-SEC-12]
    pub fn seal_close(&mut self, aad: &[u8]) -> Result<Vec<u8>, MtlsError> {
        let ct = self.seal_typed(TYPE_CLOSE, aad, &[])?;
        self.closed = true;
        Ok(ct)
    }

    /// The number of times this direction has rekeyed.
    pub fn epoch(&self) -> u64 {
        self.dir.epoch
    }

    fn seal_typed(&mut self, kind: u8, aad: &[u8], data: &[u8]) -> Result<Vec<u8>, MtlsError> {
        if self.closed {
            return Err(MtlsError::StreamClosed);
        }
        let mut plaintext = Vec::with_capacity(1 + data.len());
        plaintext.push(kind);
        plaintext.extend_from_slice(data);
        let ct = self
            .dir
            .aead
            .seal(&nonce_for(self.dir.seq), aad, &plaintext)
            .map_err(MtlsError::Crypto)?;
        self.dir.advance(data.len())?;
        Ok(ct)
    }
}

/// The receiving half of a session's record stream.
pub struct RecordOpener {
    dir: Direction,
    closed: bool,
}

impl RecordOpener {
    /// Open the next record, data or end-of-stream. A reordered, replayed, or tampered
    /// record fails the tag check and does not advance the stream; so does anything
    /// after the end-of-stream marker.
    pub fn open_record(&mut self, aad: &[u8], ciphertext: &[u8]) -> Result<Record, MtlsError> {
        if self.closed {
            return Err(MtlsError::StreamClosed);
        }
        let mut pt = self
            .dir
            .aead
            .open(&nonce_for(self.dir.seq), aad, ciphertext)
            .map_err(MtlsError::Crypto)?;
        let record = match pt.first() {
            Some(&TYPE_DATA) => {
                pt.remove(0);
                Record::Data(pt)
            }
            Some(&TYPE_CLOSE) if pt.len() == 1 => {
                self.closed = true;
                Record::Close
            }
            _ => return Err(MtlsError::Protocol("unknown record content type".into())),
        };
        let len = match &record {
            Record::Data(d) => d.len(),
            Record::Close => 0,
        };
        self.dir.advance(len)?;
        Ok(record)
    }

    /// Open the next record, which must be data: the end-of-stream marker here is
    /// [`MtlsError::StreamClosed`].
    pub fn open(&mut self, aad: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, MtlsError> {
        match self.open_record(aad, ciphertext)? {
            Record::Data(d) => Ok(d),
            Record::Close => Err(MtlsError::StreamClosed),
        }
    }

    /// Whether the peer's end-of-stream marker has been read.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// The number of times this direction has rekeyed.
    pub fn epoch(&self) -> u64 {
        self.dir.epoch
    }
}

impl Session {
    /// Derive this session's `(sealer, opener)` for the given role. Both ends of
    /// the same session produce complementary halves: what the initiator seals,
    /// the responder opens, and vice versa.
    pub fn records(&self, role: Role) -> (RecordSealer, RecordOpener) {
        self.records_with_limits(role, RekeyLimits::default())
    }

    /// [`Session::records`] with explicit rekey limits. Both ends must use the same
    /// limits; this exists so tests can exercise rekeying without 2^24 records.
    pub fn records_with_limits(
        &self,
        role: Role,
        limits: RekeyLimits,
    ) -> (RecordSealer, RecordOpener) {
        let traffic = self.traffic_secret();
        let c2s = dir_key(traffic, LABEL_C2S);
        let s2c = dir_key(traffic, LABEL_S2C);
        let (send, recv) = match role {
            Role::Initiator => (c2s, s2c),
            Role::Responder => (s2c, c2s),
        };
        (
            RecordSealer {
                dir: Direction::new(send, limits),
                closed: false,
            },
            RecordOpener {
                dir: Direction::new(recv, limits),
                closed: false,
            },
        )
    }
}
