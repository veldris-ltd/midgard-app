//! Byte framing for the handshake: a compact, canonical, length-prefixed binary
//! encoding of every [`HandshakeMessage`], a [`TcpChannel`] that carries frames
//! over a real socket, and step drivers that run each side of the handshake over
//! any [`Channel`]. No serde — hand-rolled to keep the dependency surface minimal
//! (the sovereign/air-gap posture). Decoding fails closed on any malformed input.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::TcpStream;

use policy::{Compartment, Purpose, Role};
use pqc_agility::{
    Capabilities, Ciphertext, KemPublicKey, KemSuiteId, SigId, Signature, VerifyingKey,
};

use crate::channel::Channel;
use crate::error::{MtlsError, TransportError};
use crate::handshake::{
    ClientHello, ClientKeyExchange, Finished, HandshakeMessage, Initiator, Responder, ServerHello,
};
use crate::identity::{IdentityCert, ServiceId, TrustAnchor, WorkloadIdentity};
use crate::posture::Posture;
use crate::session::Session;
use policy::ClassificationLevel;

/// Upper bound on a single frame body. Handshake messages are a few KB even with
/// ML-DSA-87; this bounds memory against a hostile length prefix.
const MAX_FRAME: usize = 1 << 20;

// ---- encoding primitives --------------------------------------------------

fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    put_u32(o, b.len() as u32);
    o.extend_from_slice(b);
}
fn put_str(o: &mut Vec<u8>, s: &str) {
    put_bytes(o, s.as_bytes());
}

/// A cursor over a frame body; every read is bounds-checked and fails closed.
struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], TransportError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| TransportError::Malformed("length overflow".into()))?;
        if end > self.b.len() {
            return Err(TransportError::Malformed("unexpected end of frame".into()));
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, TransportError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, TransportError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn bytes(&mut self) -> Result<Vec<u8>, TransportError> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn arr32(&mut self) -> Result<[u8; 32], TransportError> {
        let s = self.take(32)?;
        let mut a = [0u8; 32];
        a.copy_from_slice(s);
        Ok(a)
    }
    fn string(&mut self) -> Result<String, TransportError> {
        let b = self.bytes()?;
        String::from_utf8(b).map_err(|_| TransportError::Malformed("invalid utf-8".into()))
    }
    fn finish(self) -> Result<(), TransportError> {
        if self.pos == self.b.len() {
            Ok(())
        } else {
            Err(TransportError::Malformed("trailing bytes".into()))
        }
    }
}

fn dec_sig_id(r: &mut Reader) -> Result<SigId, TransportError> {
    SigId::from_u8(r.u8()?).ok_or_else(|| TransportError::Malformed("unknown sig id".into()))
}
fn dec_suite(r: &mut Reader) -> Result<KemSuiteId, TransportError> {
    KemSuiteId::from_u8(r.u8()?)
        .ok_or_else(|| TransportError::Malformed("unknown kem suite".into()))
}

// ---- type codecs ----------------------------------------------------------

fn enc_sig(o: &mut Vec<u8>, s: &Signature) {
    o.push(s.alg().as_u8());
    put_bytes(o, s.as_bytes());
}
fn dec_sig(r: &mut Reader) -> Result<Signature, TransportError> {
    let alg = dec_sig_id(r)?;
    Ok(Signature::new(alg, r.bytes()?))
}

fn enc_vk(o: &mut Vec<u8>, vk: &VerifyingKey) {
    o.push(vk.alg().as_u8());
    put_bytes(o, vk.as_bytes());
}
fn dec_vk(r: &mut Reader) -> Result<VerifyingKey, TransportError> {
    let alg = dec_sig_id(r)?;
    Ok(VerifyingKey::new(alg, r.bytes()?))
}

fn enc_kem_pk(o: &mut Vec<u8>, pk: &KemPublicKey) {
    o.push(pk.suite().as_u8());
    put_bytes(o, pk.as_bytes());
}
fn dec_kem_pk(r: &mut Reader) -> Result<KemPublicKey, TransportError> {
    let suite = dec_suite(r)?;
    Ok(KemPublicKey::new(suite, r.bytes()?))
}

fn enc_ct(o: &mut Vec<u8>, ct: &Ciphertext) {
    o.push(ct.suite().as_u8());
    put_bytes(o, ct.as_bytes());
}
fn dec_ct(r: &mut Reader) -> Result<Ciphertext, TransportError> {
    let suite = dec_suite(r)?;
    Ok(Ciphertext::new(suite, r.bytes()?))
}

fn enc_identity(o: &mut Vec<u8>, id: &WorkloadIdentity) {
    put_str(o, &id.id.0);
    put_u32(o, id.roles.len() as u32);
    for r in &id.roles {
        put_str(o, &r.0);
    }
    o.push(id.sig_alg.as_u8());
    enc_vk(o, &id.verifying_key);
}
fn dec_identity(r: &mut Reader) -> Result<WorkloadIdentity, TransportError> {
    let id = ServiceId(r.string()?);
    let n = r.u32()? as usize;
    let mut roles = BTreeSet::new();
    for _ in 0..n {
        roles.insert(Role(r.string()?));
    }
    let sig_alg = dec_sig_id(r)?;
    let verifying_key = dec_vk(r)?;
    Ok(WorkloadIdentity {
        id,
        roles,
        sig_alg,
        verifying_key,
    })
}

fn enc_cert(o: &mut Vec<u8>, c: &IdentityCert) {
    enc_identity(o, &c.identity);
    o.push(c.ca_sig_alg.as_u8());
    enc_sig(o, &c.ca_signature);
}
fn dec_cert(r: &mut Reader) -> Result<IdentityCert, TransportError> {
    let identity = dec_identity(r)?;
    let ca_sig_alg = dec_sig_id(r)?;
    let ca_signature = dec_sig(r)?;
    Ok(IdentityCert {
        identity,
        ca_sig_alg,
        ca_signature,
    })
}

// ---- identity files -------------------------------------------------------

/// File magic of a stored [`IdentityCert`].
pub const CERT_MAGIC: &[u8; 7] = b"MGCERT1";
/// File magic of a stored [`TrustAnchor`].
pub const ANCHOR_MAGIC: &[u8; 9] = b"MGANCHOR1";

/// Encode a workload certificate for storage: the magic, then the same canonical
/// encoding the handshake carries, so a service loads exactly what it will present.
/// [NFR-SEC-1]
pub fn encode_cert(cert: &IdentityCert) -> Vec<u8> {
    let mut o = CERT_MAGIC.to_vec();
    enc_cert(&mut o, cert);
    o
}

/// Decode a stored workload certificate. A wrong magic, truncated or trailing input,
/// or a verifying key whose algorithm is not the one the identity declares is refused.
pub fn decode_cert(bytes: &[u8]) -> Result<IdentityCert, TransportError> {
    let body = bytes
        .strip_prefix(CERT_MAGIC.as_slice())
        .ok_or_else(|| TransportError::Malformed("not a certificate file".into()))?;
    let mut r = Reader::new(body);
    let cert = dec_cert(&mut r)?;
    r.finish()?;
    if cert.identity.verifying_key.alg() != cert.identity.sig_alg {
        return Err(TransportError::Malformed(
            "certificate key algorithm does not match its identity".into(),
        ));
    }
    Ok(cert)
}

/// Encode a trust anchor for storage: the magic, the CA algorithm and key, and the
/// revoked fingerprints, so the CRL travels with the anchor. [RC-35]
pub fn encode_anchor(anchor: &TrustAnchor) -> Vec<u8> {
    let mut o = ANCHOR_MAGIC.to_vec();
    o.push(anchor.ca_alg.as_u8());
    enc_vk(&mut o, &anchor.ca_key);
    put_u32(&mut o, anchor.revoked.len() as u32);
    for fingerprint in &anchor.revoked {
        o.extend_from_slice(fingerprint);
    }
    o
}

/// Decode a stored trust anchor, revocations included. A wrong magic, truncated or
/// trailing input, or a CA key whose algorithm is not the declared one is refused.
pub fn decode_anchor(bytes: &[u8]) -> Result<TrustAnchor, TransportError> {
    let body = bytes
        .strip_prefix(ANCHOR_MAGIC.as_slice())
        .ok_or_else(|| TransportError::Malformed("not a trust anchor file".into()))?;
    let mut r = Reader::new(body);
    let ca_alg = dec_sig_id(&mut r)?;
    let ca_key = dec_vk(&mut r)?;
    if ca_key.alg() != ca_alg {
        return Err(TransportError::Malformed(
            "trust anchor key algorithm does not match its declared algorithm".into(),
        ));
    }
    let n = r.u32()? as usize;
    let mut revoked = BTreeSet::new();
    for _ in 0..n {
        revoked.insert(r.arr32()?);
    }
    r.finish()?;
    Ok(TrustAnchor::new(ca_alg, ca_key).with_revocations(revoked))
}

fn enc_caps(o: &mut Vec<u8>, c: &Capabilities) {
    put_u32(o, c.kem_suites.len() as u32);
    for s in &c.kem_suites {
        o.push(s.as_u8());
    }
    put_u32(o, c.sig_algs.len() as u32);
    for a in &c.sig_algs {
        o.push(a.as_u8());
    }
}
fn dec_caps(r: &mut Reader) -> Result<Capabilities, TransportError> {
    let nk = r.u32()? as usize;
    let mut kem_suites = Vec::with_capacity(nk);
    for _ in 0..nk {
        kem_suites.push(dec_suite(r)?);
    }
    let ns = r.u32()? as usize;
    let mut sig_algs = Vec::with_capacity(ns);
    for _ in 0..ns {
        sig_algs.push(dec_sig_id(r)?);
    }
    Ok(Capabilities {
        kem_suites,
        sig_algs,
    })
}

fn enc_posture(o: &mut Vec<u8>, p: &Posture) {
    o.push(p.level.0);
    put_u32(o, p.compartments.len() as u32);
    for c in &p.compartments {
        put_str(o, &c.0);
    }
}
fn dec_posture(r: &mut Reader) -> Result<Posture, TransportError> {
    let level = ClassificationLevel(r.u8()?);
    let n = r.u32()? as usize;
    let mut compartments = BTreeSet::new();
    for _ in 0..n {
        compartments.insert(Compartment(r.string()?));
    }
    Ok(Posture {
        level,
        compartments,
    })
}

// ---- message codec --------------------------------------------------------

const TAG_CLIENT_HELLO: u8 = 1;
const TAG_SERVER_HELLO: u8 = 2;
const TAG_CKE: u8 = 3;
const TAG_FINISHED: u8 = 4;

/// Encode a handshake message to its canonical byte body (no length prefix).
pub fn encode_message(m: &HandshakeMessage) -> Vec<u8> {
    let mut o = Vec::new();
    match m {
        HandshakeMessage::ClientHello(m) => {
            o.push(TAG_CLIENT_HELLO);
            o.extend_from_slice(&m.nonce_c);
            enc_caps(&mut o, &m.capabilities);
            put_str(&mut o, &m.purpose.0);
            enc_posture(&mut o, &m.posture);
        }
        HandshakeMessage::ServerHello(m) => {
            o.push(TAG_SERVER_HELLO);
            o.extend_from_slice(&m.nonce_s);
            o.push(m.suite.as_u8());
            enc_kem_pk(&mut o, &m.kem_pk);
            enc_cert(&mut o, &m.cert);
            o.push(m.sig_alg.as_u8());
            enc_sig(&mut o, &m.sig_r);
        }
        HandshakeMessage::ClientKeyExchange(m) => {
            o.push(TAG_CKE);
            enc_ct(&mut o, &m.ct);
            match &m.cert {
                Some(c) => {
                    o.push(1);
                    enc_cert(&mut o, c);
                }
                None => o.push(0),
            }
            o.push(m.sig_alg.as_u8());
            enc_sig(&mut o, &m.sig_i);
            o.extend_from_slice(&m.finished_i);
        }
        HandshakeMessage::Finished(m) => {
            o.push(TAG_FINISHED);
            o.extend_from_slice(&m.finished_r);
        }
    }
    o
}

/// Decode a handshake message from a frame body. Fails closed on any defect.
pub fn decode_message(body: &[u8]) -> Result<HandshakeMessage, TransportError> {
    let mut r = Reader::new(body);
    let tag = r.u8()?;
    let msg = match tag {
        TAG_CLIENT_HELLO => {
            let nonce_c = r.arr32()?;
            let capabilities = dec_caps(&mut r)?;
            let purpose = Purpose(r.string()?);
            let posture = dec_posture(&mut r)?;
            HandshakeMessage::ClientHello(ClientHello {
                nonce_c,
                capabilities,
                purpose,
                posture,
            })
        }
        TAG_SERVER_HELLO => {
            let nonce_s = r.arr32()?;
            let suite = dec_suite(&mut r)?;
            let kem_pk = dec_kem_pk(&mut r)?;
            let cert = dec_cert(&mut r)?;
            let sig_alg = dec_sig_id(&mut r)?;
            let sig_r = dec_sig(&mut r)?;
            HandshakeMessage::ServerHello(ServerHello {
                nonce_s,
                suite,
                kem_pk,
                cert,
                sig_alg,
                sig_r,
            })
        }
        TAG_CKE => {
            let ct = dec_ct(&mut r)?;
            let cert = match r.u8()? {
                0 => None,
                1 => Some(dec_cert(&mut r)?),
                _ => return Err(TransportError::Malformed("bad cert presence".into())),
            };
            let sig_alg = dec_sig_id(&mut r)?;
            let sig_i = dec_sig(&mut r)?;
            let finished_i = r.arr32()?;
            HandshakeMessage::ClientKeyExchange(ClientKeyExchange {
                ct,
                cert,
                sig_alg,
                sig_i,
                finished_i,
            })
        }
        TAG_FINISHED => {
            let finished_r = r.arr32()?;
            HandshakeMessage::Finished(Finished { finished_r })
        }
        _ => return Err(TransportError::Malformed("unknown message tag".into())),
    };
    r.finish()?;
    Ok(msg)
}

// ---- TCP channel ----------------------------------------------------------

/// A [`Channel`] that frames handshake messages over a [`TcpStream`]
/// (u32-LE length prefix + canonical body).
pub struct TcpChannel {
    stream: TcpStream,
}

impl TcpChannel {
    /// Wrap a connected stream.
    pub fn new(stream: TcpStream) -> Self {
        Self { stream }
    }
    /// Borrow the underlying stream (e.g. to hand records to the same socket).
    pub fn stream_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }
    /// Consume and return the underlying stream after the handshake.
    pub fn into_stream(self) -> TcpStream {
        self.stream
    }
}

fn io(e: std::io::Error) -> TransportError {
    TransportError::Io(e.to_string())
}

/// Write a length-prefixed frame (`u32-LE len ‖ body`). The framing used for both
/// handshake messages and, post-handshake, sealed records.
pub fn write_frame(w: &mut impl Write, body: &[u8]) -> Result<(), TransportError> {
    w.write_all(&(body.len() as u32).to_le_bytes())
        .map_err(io)?;
    w.write_all(body).map_err(io)?;
    w.flush().map_err(io)?;
    Ok(())
}

/// Read one length-prefixed frame, bounding the body length against a hostile
/// prefix.
pub fn read_frame(r: &mut impl Read) -> Result<Vec<u8>, TransportError> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).map_err(io)?;
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(TransportError::Malformed("frame too large".into()));
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body).map_err(io)?;
    Ok(body)
}

impl Channel for TcpChannel {
    fn send(&mut self, msg: HandshakeMessage) -> Result<(), TransportError> {
        write_frame(&mut self.stream, &encode_message(&msg))
    }
    fn recv(&mut self) -> Result<HandshakeMessage, TransportError> {
        decode_message(&read_frame(&mut self.stream)?)
    }
}

// ---- channel-driven handshake drivers -------------------------------------

fn expect_transport<T>(r: Result<T, TransportError>) -> Result<T, MtlsError> {
    r.map_err(MtlsError::Transport)
}

/// Drive the initiator (client) side of the handshake over a channel.
pub fn run_initiator(mut init: Initiator, ch: &mut dyn Channel) -> Result<Session, MtlsError> {
    let hello = init.start();
    expect_transport(ch.send(HandshakeMessage::ClientHello(hello)))?;
    let sh = match expect_transport(ch.recv())? {
        HandshakeMessage::ServerHello(sh) => sh,
        _ => return Err(MtlsError::Protocol("expected ServerHello".into())),
    };
    let cke = init.on_server_hello(sh)?;
    expect_transport(ch.send(HandshakeMessage::ClientKeyExchange(cke)))?;
    let fin = match expect_transport(ch.recv())? {
        HandshakeMessage::Finished(f) => f,
        _ => return Err(MtlsError::Protocol("expected Finished".into())),
    };
    init.on_finished(fin)
}

/// Drive the responder (server) side of the handshake over a channel.
pub fn run_responder(mut resp: Responder, ch: &mut dyn Channel) -> Result<Session, MtlsError> {
    let hello = match expect_transport(ch.recv())? {
        HandshakeMessage::ClientHello(c) => c,
        _ => return Err(MtlsError::Protocol("expected ClientHello".into())),
    };
    let sh = resp.on_client_hello(hello)?;
    expect_transport(ch.send(HandshakeMessage::ServerHello(sh)))?;
    let cke = match expect_transport(ch.recv())? {
        HandshakeMessage::ClientKeyExchange(c) => c,
        _ => return Err(MtlsError::Protocol("expected ClientKeyExchange".into())),
    };
    let (fin, session) = resp.on_client_key_exchange(cke)?;
    expect_transport(ch.send(HandshakeMessage::Finished(fin)))?;
    Ok(session)
}
