//! The real feed transport: a **PQ-mTLS listener** that ingests signed observations
//! from live sources, and a **feed SDK** (`FeedClient`) a source uses to push them.
//!
//! A feed source connects over a mutually-authenticated, hybrid post-quantum channel
//! (the same `pq-mtls` transport the rest of the suite uses), then streams
//! [`SignedObservation`]s as sealed records. The listener opens each record,
//! reconstructs the observation, and runs it through [`FeedGateway::authenticate`] —
//! so an unknown feed, an algorithm mismatch, a bad signature, or a tampered record
//! never yields a [`VerifiedObservation`] and never reaches fusion. Admission stays
//! type-level; the transport only moves bytes. [FR-H-3] [NFR-PQ-1] [NFR-PQ-3]
//!
//! The `pq-mtls` handshake parameters (identity cert, signing key, trust anchor,
//! and — server side — the channel PDP/resource) are supplied by the caller as
//! `pq_mtls` types, so this crate stays decoupled from the IdP wiring.

use std::io;
use std::net::{TcpListener, TcpStream};

use pq_mtls::{
    read_frame, run_initiator, run_responder, write_frame, Initiator, MtlsError, RecordOpener,
    RecordSealer, Responder, Role, TcpChannel, TransportError,
};

use heimdall_picture::{Observation, PictureError};
use pqc_agility::{SigId, Signature};

use crate::authenticator::{FeedGateway, SignedObservation, VerifiedObservation};
use crate::error::FeedError;

/// AAD binding every feed record to this channel context; sender and receiver must
/// agree on it, so a record replayed from another channel fails the AEAD tag.
pub const FEED_AAD: &[u8] = b"heimdall-feed/1";

/// A failure anywhere on the feed transport path.
#[derive(Debug)]
pub enum FeedTransportError {
    /// The PQ-mTLS session (handshake or record layer) failed.
    Mtls(MtlsError),
    /// Byte framing failed.
    Transport(TransportError),
    /// A received record could not be decoded into a signed observation.
    Malformed(String),
    /// The observation payload failed to decode.
    Decode(PictureError),
    /// The observation was not admitted (unknown feed / bad signature / mismatch).
    Rejected(FeedError),
}

impl std::fmt::Display for FeedTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedTransportError::Mtls(e) => write!(f, "pq-mtls: {e:?}"),
            FeedTransportError::Transport(e) => write!(f, "transport: {e:?}"),
            FeedTransportError::Malformed(m) => write!(f, "malformed feed record: {m}"),
            FeedTransportError::Decode(e) => write!(f, "observation decode: {e}"),
            FeedTransportError::Rejected(e) => write!(f, "feed rejected: {e}"),
        }
    }
}
impl std::error::Error for FeedTransportError {}

impl From<MtlsError> for FeedTransportError {
    fn from(e: MtlsError) -> Self {
        FeedTransportError::Mtls(e)
    }
}
impl From<TransportError> for FeedTransportError {
    fn from(e: TransportError) -> Self {
        FeedTransportError::Transport(e)
    }
}
impl From<PictureError> for FeedTransportError {
    fn from(e: PictureError) -> Self {
        FeedTransportError::Decode(e)
    }
}
impl From<FeedError> for FeedTransportError {
    fn from(e: FeedError) -> Self {
        FeedTransportError::Rejected(e)
    }
}

// ---- wire codec for a SignedObservation -----------------------------------

fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    put_u32(o, b.len() as u32);
    o.extend_from_slice(b);
}

/// Encode a signed observation to a record body: the observation's canonical bytes,
/// then its signature (algorithm id + bytes).
pub fn encode_signed_observation(s: &SignedObservation) -> Vec<u8> {
    let mut o = Vec::new();
    put_bytes(&mut o, &s.observation.canonical_bytes());
    o.push(s.signature.alg().as_u8());
    put_bytes(&mut o, s.signature.as_bytes());
    o
}

fn take<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], FeedTransportError> {
    let end = pos
        .checked_add(n)
        .ok_or_else(|| FeedTransportError::Malformed("length overflow".into()))?;
    let s = buf
        .get(*pos..end)
        .ok_or_else(|| FeedTransportError::Malformed("truncated feed record".into()))?;
    *pos = end;
    Ok(s)
}
fn take_u32(buf: &[u8], pos: &mut usize) -> Result<usize, FeedTransportError> {
    let s = take(buf, pos, 4)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

/// Decode a record body back into a signed observation. Fails closed.
pub fn decode_signed_observation(buf: &[u8]) -> Result<SignedObservation, FeedTransportError> {
    let mut pos = 0;
    let obs_len = take_u32(buf, &mut pos)?;
    let obs_bytes = take(buf, &mut pos, obs_len)?;
    let observation = Observation::from_canonical_bytes(obs_bytes)?;
    let alg = SigId::from_u8(*take(buf, &mut pos, 1)?.first().expect("one byte taken"))
        .ok_or_else(|| FeedTransportError::Malformed("unknown signature algorithm".into()))?;
    let sig_len = take_u32(buf, &mut pos)?;
    let sig_bytes = take(buf, &mut pos, sig_len)?.to_vec();
    if pos != buf.len() {
        return Err(FeedTransportError::Malformed("trailing bytes".into()));
    }
    Ok(SignedObservation {
        observation,
        signature: Signature::new(alg, sig_bytes),
    })
}

// ---- feed SDK (source side) -----------------------------------------------

/// The feed SDK: a real source connects to the gateway over PQ-mTLS and pushes
/// signed observations. Construct with [`FeedClient::connect`], then [`send`] each
/// signed observation. [FR-H-3]
///
/// [`send`]: FeedClient::send
pub struct FeedClient {
    stream: TcpStream,
    sealer: RecordSealer,
    #[allow(dead_code)]
    opener: RecordOpener,
}

impl FeedClient {
    /// Connect to a feed listener at `stream`, running the PQ-mTLS initiator
    /// handshake (`initiator` carries this source's identity + trust anchor).
    pub fn connect(stream: TcpStream, initiator: Initiator) -> Result<Self, FeedTransportError> {
        let mut ch = TcpChannel::new(stream);
        let session = run_initiator(initiator, &mut ch)?;
        let stream = ch.into_stream();
        let (sealer, opener) = session.records(Role::Initiator);
        Ok(Self {
            stream,
            sealer,
            opener,
        })
    }

    /// Seal and transmit one signed observation over the session.
    pub fn send(&mut self, signed: &SignedObservation) -> Result<(), FeedTransportError> {
        let body = encode_signed_observation(signed);
        let sealed = self.sealer.seal(FEED_AAD, &body)?;
        write_frame(&mut self.stream, &sealed)?;
        Ok(())
    }
}

// ---- feed listener (gateway side) -----------------------------------------

/// A PQ-mTLS feed listener: binds a socket and accepts feed-source connections.
pub struct FeedListener {
    listener: TcpListener,
}

impl FeedListener {
    /// Bind a listening socket.
    pub fn bind(addr: &str) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr)?,
        })
    }

    /// Wrap an already-bound listener (e.g. on an ephemeral port).
    pub fn from_listener(listener: TcpListener) -> Self {
        Self { listener }
    }

    /// The bound address (useful with an ephemeral `127.0.0.1:0` bind).
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }

    /// Accept the next feed connection's raw stream.
    pub fn accept(&self) -> io::Result<TcpStream> {
        let (stream, _peer) = self.listener.accept()?;
        Ok(stream)
    }
}

/// Terminate PQ-mTLS on `stream` (as `responder`), then read signed observations,
/// authenticate each through `gateway`, and hand every [`VerifiedObservation`] to
/// `sink`. Returns the number admitted. **Fails closed**: an unknown feed, a bad
/// signature, an algorithm mismatch, a tampered record, or a malformed frame returns
/// an error and no further observations are read — an unsigned/unauthenticated feed
/// is rejected, not fused. [FR-H-3] [NFR-PQ-3] [RC-26]
pub fn serve_feed_connection<F>(
    stream: TcpStream,
    responder: Responder,
    gateway: &FeedGateway,
    mut sink: F,
) -> Result<usize, FeedTransportError>
where
    F: FnMut(VerifiedObservation),
{
    let mut ch = TcpChannel::new(stream);
    let session = run_responder(responder, &mut ch)?;
    let mut stream = ch.into_stream();
    let (_sealer, mut opener) = session.records(Role::Responder);

    let mut admitted = 0usize;
    loop {
        let frame = match read_frame(&mut stream) {
            Ok(f) if !f.is_empty() => f,
            _ => break, // clean close / EOF
        };
        // Open the record (a tampered/replayed record fails the AEAD tag here) …
        let record = opener.open(FEED_AAD, &frame)?;
        // … reconstruct the signed observation …
        let signed = decode_signed_observation(&record)?;
        // … and admit it only if its source signature verifies. A rejection stops
        // the connection: an untrusted source is cut off, not merely skipped.
        let verified = gateway.authenticate(&signed)?;
        sink(verified);
        admitted += 1;
    }
    Ok(admitted)
}
