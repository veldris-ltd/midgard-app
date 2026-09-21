//! The real replication transport: a [`DeltaLink`] backed by a `pq-mtls`
//! [`Session`]. Signed deltas ride the same mutually-authenticated, hybrid
//! post-quantum channel the rest of the suite uses — each delta is sealed as a
//! record and framed on the wire. A [`PeerRegistry`] tracks the replication
//! verifying key for each peer node, and [`replicate_round`] drives one
//! export→send→receive→verify→merge exchange. Signature verify + merge are
//! unchanged: the record layer adds confidentiality/replay protection on top of the
//! per-delta source signature. [FR-H-6] [NFR-PQ-1] [NFR-PQ-3]

use std::collections::BTreeMap;
use std::net::TcpStream;

use pq_mtls::{read_frame, write_frame, RecordOpener, RecordSealer, Role, Session};
use pqc_agility::{SigId, Signature, SigningKey, VerifyingKey};

use heimdall_picture::Picture;

use crate::error::SyncError;
use crate::link::DeltaLink;
use crate::replicator::{Replicator, SignedDelta};

/// AAD binding every delta record to this replication channel context.
pub const DELTA_AAD: &[u8] = b"heimdall-delta/1";

// ---- wire codec for a SignedDelta -----------------------------------------

fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    put_u32(o, b.len() as u32);
    o.extend_from_slice(b);
}

/// Encode a signed delta to a record body: the canonical delta bytes, then the
/// signature (algorithm id + bytes).
pub fn encode_signed_delta(d: &SignedDelta) -> Vec<u8> {
    let mut o = Vec::new();
    put_bytes(&mut o, &d.bytes);
    o.push(d.signature.alg().as_u8());
    put_bytes(&mut o, d.signature.as_bytes());
    o
}

fn take<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], SyncError> {
    let end = pos
        .checked_add(n)
        .ok_or_else(|| SyncError::Transport("delta length overflow".into()))?;
    let s = buf
        .get(*pos..end)
        .ok_or_else(|| SyncError::Transport("truncated delta record".into()))?;
    *pos = end;
    Ok(s)
}
fn take_u32(buf: &[u8], pos: &mut usize) -> Result<usize, SyncError> {
    let s = take(buf, pos, 4)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

/// Decode a record body back into a signed delta. Fails closed.
pub fn decode_signed_delta(buf: &[u8]) -> Result<SignedDelta, SyncError> {
    let mut pos = 0;
    let n = take_u32(buf, &mut pos)?;
    let bytes = take(buf, &mut pos, n)?.to_vec();
    let alg = SigId::from_u8(*take(buf, &mut pos, 1)?.first().expect("byte taken"))
        .ok_or_else(|| SyncError::Transport("unknown delta signature algorithm".into()))?;
    let sig_len = take_u32(buf, &mut pos)?;
    let sig_bytes = take(buf, &mut pos, sig_len)?.to_vec();
    if pos != buf.len() {
        return Err(SyncError::Transport("trailing delta bytes".into()));
    }
    Ok(SignedDelta {
        bytes,
        signature: Signature::new(alg, sig_bytes),
    })
}

// ---- the pq-mtls-backed link ----------------------------------------------

/// A [`DeltaLink`] over a real `pq-mtls` session: deltas are sealed records framed
/// on the underlying TCP stream. Construct from an established [`Session`] and its
/// stream after the handshake.
pub struct SessionLink {
    stream: TcpStream,
    sealer: RecordSealer,
    opener: RecordOpener,
}

impl SessionLink {
    /// Wrap an established session (and its post-handshake stream) as a delta link.
    /// `role` fixes the record send/receive keys — both ends pass their own role.
    pub fn new(session: &Session, stream: TcpStream, role: Role) -> Self {
        let (sealer, opener) = session.records(role);
        Self {
            stream,
            sealer,
            opener,
        }
    }
}

impl DeltaLink for SessionLink {
    fn send(&mut self, delta: SignedDelta) -> Result<(), SyncError> {
        let body = encode_signed_delta(&delta);
        let sealed = self
            .sealer
            .seal(DELTA_AAD, &body)
            .map_err(|e| SyncError::Transport(format!("{e:?}")))?;
        write_frame(&mut self.stream, &sealed).map_err(|e| SyncError::Transport(format!("{e:?}")))
    }

    fn recv(&mut self) -> Result<SignedDelta, SyncError> {
        let frame =
            read_frame(&mut self.stream).map_err(|e| SyncError::Transport(format!("{e:?}")))?;
        let body = self
            .opener
            .open(DELTA_AAD, &frame)
            .map_err(|e| SyncError::Transport(format!("{e:?}")))?;
        decode_signed_delta(&body)
    }
}

// ---- peer / key management -------------------------------------------------

/// Maps each peer node's id to the replication verifying key its deltas are signed
/// under. A delta is only merged after verifying against the key registered here for
/// its source — an unknown or wrong-key peer is rejected. [FR-H-6]
#[derive(Debug, Default, Clone)]
pub struct PeerRegistry {
    keys: BTreeMap<String, VerifyingKey>,
}

impl PeerRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a peer node's replication verifying key.
    pub fn register(&mut self, node_id: impl Into<String>, key: VerifyingKey) {
        self.keys.insert(node_id.into(), key);
    }

    /// The verifying key for `node_id`, if the peer is known.
    pub fn key_for(&self, node_id: &str) -> Option<&VerifyingKey> {
        self.keys.get(node_id)
    }

    /// Number of known peers.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether no peers are registered.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

// ---- replication loop ------------------------------------------------------

/// One replication round with a peer: export the local `picture` as a signed delta
/// and send it, then receive the peer's delta, verify it against `peer_key`, and
/// merge it. Returns the number of entities inserted/updated from the peer. Fails
/// closed — an unverified peer delta merges nothing. [FR-H-6] [NFR-PQ-3] [RC-26]
pub fn replicate_round(
    link: &mut dyn DeltaLink,
    repl: &Replicator,
    signing_key: &SigningKey,
    peer_key: &VerifyingKey,
    picture: &mut Picture,
) -> Result<usize, SyncError> {
    let outgoing = repl.export(signing_key, picture)?;
    link.send(outgoing)?;
    let incoming = link.recv()?;
    repl.import(peer_key, &incoming, picture)
}
