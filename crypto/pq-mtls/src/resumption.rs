//! Abbreviated (session-resumption) handshake — TLS 1.3-style PSK resumption for [NFR-SEC-1]
//! PQ-mTLS. After a full mutual handshake, the responder issues the initiator an
//! opaque **session ticket**: the resumption-master secret (RMS) plus the peer's
//! certificate and what the handshake authorised it for, AEAD-sealed under a
//! responder-held ticket key. The responder delivers it inside the session's record
//! layer (sealed under [`NEW_TICKET_AAD`]), never in clear. A later connection can
//! then *resume* without the ML-DSA signature flights — the expensive part of the
//! handshake is amortised. [RT-25]
//!
//! Security model (mirrors TLS 1.3 `psk_dhe_ke`):
//! - The initiator proves possession of the RMS with a **binder** MAC over the whole
//!   ResumeHello, its fresh KEM share included. Only a party that completed the
//!   original mutually-authenticated session knows the RMS, so a valid binder
//!   authenticates the client — no signature needed.
//! - The responder checks, in this order: the ticket opens, the ticket is in date,
//!   the binder, the single-use replay marker, the client certificate, and the
//!   policy. A ResumeHello with a bad binder never consumes the ticket.
//!   [VLD-ANB-MIDGARD-0046]
//! - Every resumption runs a **fresh KEM exchange** in the ticket's suite: the
//!   initiator sends an ephemeral public key and the responder encapsulates to it.
//!   The resumed traffic secret is HKDF with the transcript hash as the (public) salt
//!   and `RMS || shared secret` as the (secret) input keying material, so a resumed
//!   leg has forward secrecy like a full handshake, minus the signatures: a later
//!   compromise of the ticket key or the RMS does not reveal its traffic.
//!   [VLD-ANB-MIDGARD-0046]
//! - The responder proves possession of the ticket key (it opened the ticket), the
//!   RMS and the shared secret with its **finished** MAC — authenticating the server
//!   to the client.
//! - A resumption is **re-checked against the live revocation state and the
//!   certificate's validity window**, on both sides: the ticket carries the client's
//!   full certificate and the responder re-runs [`verify_cert`] on it (CA signature,
//!   validity, revocation list), so a certificate revoked or expired since the ticket
//!   was issued does not resume, and a ticket never outlives the certificate it was
//!   issued for. [MG-SEC-03] [MG-SEC-02] [VLD-ANB-MIDGARD-0046]
//! - The ticket carries the clearance and purpose the full handshake actually
//!   authorised — the lower of the certificate's grant and the request — so a
//!   resumed connection is re-authorised as the original was. [MG-SEC-04]
//! - Tickets are time-boxed (a TTL the responder enforces), name the [`TicketKey`]
//!   that sealed them in their first byte so the responder can rotate keys and keep
//!   the previous one, and are single-use when the caller supplies a replay guard
//!   (see the gateway's session manager).
//!
//! Requires the production AEAD, so this module is gated on `backend-fips`.

use std::collections::BTreeSet;

use policy::{ClassificationLevel, Compartment, PolicyDecisionPoint, Purpose, Resource};
use pqc_agility::hash::{ct_eq, hkdf_sha256_32, hmac_sha256, sha256};
use pqc_agility::{
    Aes256GcmAead, Ciphertext, CryptoProvider, KemPublicKey, KemSuiteId, Rng, SecretArray,
    SecretBytes, GCM_NONCE_LEN,
};

use crate::error::{AuthzError, MtlsError, TransportError};
use crate::handshake::authorize;
use crate::identity::{verify_cert, IdentityCert, TrustAnchor, WorkloadIdentity};
use crate::posture::Posture;
use crate::session::{Authorization, PeerCert, Session};
use crate::wire::{decode_cert, encode_cert, read_handshake_frame, write_frame};

// Version 3: the ticket names its key and carries the full client certificate, and
// the resumption runs a fresh KEM exchange. Version-2 tickets and messages are
// refused, and the client falls back to a full handshake. [VLD-ANB-MIDGARD-0046]
const TICKET_AAD: &[u8] = b"midgard-pqmtls-ticket-v3";
const RESUME_INFO: &[u8] = b"midgard-pqmtls-resume-traffic-v3";
const BINDER_INFO: &[u8] = b"midgard-pqmtls-resume-binder-v3";
const BINDER_LABEL: &[u8] = b"resume-binder";
const SERVER_FINISHED_LABEL: &[u8] = b"resume-server-finished";

/// AAD of the record that carries a new session ticket from the responder to the
/// initiator: the first record the responder seals after a full handshake.
/// [VLD-ANB-MIDGARD-0046]
pub const NEW_TICKET_AAD: &[u8] = b"midgard-pqmtls-new-ticket-v2";

/// Tag for the very first frame on a connection, so the responder knows whether a
/// full handshake or a resumption follows. [RT-25]
pub const MODE_FULL: u8 = 0x01;
/// Resumption connection mode (see [`MODE_FULL`]).
pub const MODE_RESUME: u8 = 0x02;

/// A responder-held secret that seals/opens session tickets, and the one-byte id a
/// ticket names it by. It never leaves the responder; the initiator only ever sees
/// opaque sealed bytes.
pub struct TicketKey {
    id: u8,
    key: SecretArray<32>,
}

impl TicketKey {
    /// Wrap raw key bytes (e.g. from the KMS / a rotated ticket-key schedule), with
    /// key id 0.
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            id: 0,
            key: SecretArray::new(key),
        }
    }

    /// Generate a fresh random ticket key, with key id 0.
    pub fn generate(rng: &mut dyn Rng) -> Self {
        let mut key = SecretArray::zeroed();
        rng.fill(key.expose_mut());
        Self { id: 0, key }
    }

    /// The same key under another id. A responder rotating its keys gives each new
    /// key the next id, so a ticket's first byte says which key opens it.
    pub fn with_id(mut self, id: u8) -> Self {
        self.id = id;
        self
    }

    /// The id tickets sealed under this key carry in their first byte.
    pub fn id(&self) -> u8 {
        self.id
    }

    fn aead(&self) -> Aes256GcmAead {
        Aes256GcmAead::new(self.key.expose())
    }
}

/// What the initiator keeps after a full handshake so it can resume later: the
/// opaque ticket, the RMS (kept locally, never on the wire), and the authenticated
/// server identity, certificate facts and suite to reconstruct the resumed
/// [`Session`].
pub struct ClientTicket {
    /// The opaque, responder-sealed ticket bytes.
    pub ticket: Vec<u8>,
    rms: SecretArray<32>,
    server: WorkloadIdentity,
    server_cert: PeerCert,
    suite: KemSuiteId,
}

impl ClientTicket {
    /// Build the initiator-side resumption state from a completed session and the
    /// ticket bytes the responder sent (recovered from its [`NEW_TICKET_AAD`] record).
    pub fn new(session: &Session, ticket: Vec<u8>) -> Self {
        Self {
            ticket,
            rms: session.resumption_secret(),
            server: session.peer().clone(),
            server_cert: session.peer_cert(),
            suite: session.suite(),
        }
    }

    /// The server this ticket resumes a session with.
    pub fn server(&self) -> &WorkloadIdentity {
        &self.server
    }
}

// ---- ticket contents (responder-private plaintext) ------------------------

/// The sealed contents of a ticket. Never exposed to the initiator.
struct TicketBody {
    /// The client's full certificate, re-verified on resumption.
    cert: IdentityCert,
    posture: Posture,
    purpose: Purpose,
    suite: KemSuiteId,
    issued_at: u64,
    ttl: u64,
    rms: SecretArray<32>,
}

// ---- small local codec (self-contained; does not touch the handshake enum) -

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
            .ok_or_else(|| TransportError::Malformed("ticket length overflow".into()))?;
        if end > self.b.len() {
            return Err(TransportError::Malformed("ticket truncated".into()));
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
    fn u64(&mut self) -> Result<u64, TransportError> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_le_bytes(a))
    }
    fn bytes(&mut self) -> Result<&'a [u8], TransportError> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    fn arr32(&mut self) -> Result<[u8; 32], TransportError> {
        let s = self.take(32)?;
        let mut a = [0u8; 32];
        a.copy_from_slice(s);
        Ok(a)
    }
    fn string(&mut self) -> Result<String, TransportError> {
        String::from_utf8(self.bytes()?.to_vec())
            .map_err(|_| TransportError::Malformed("utf-8".into()))
    }
    fn suite(&mut self) -> Result<KemSuiteId, TransportError> {
        KemSuiteId::from_u8(self.u8()?).ok_or_else(|| TransportError::Malformed("kem suite".into()))
    }
    fn finish(self) -> Result<(), TransportError> {
        if self.pos == self.b.len() {
            Ok(())
        } else {
            Err(TransportError::Malformed("trailing ticket bytes".into()))
        }
    }
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

fn encode_ticket_body(b: &TicketBody) -> SecretBytes {
    let mut o = Vec::new();
    put_bytes(&mut o, &encode_cert(&b.cert));
    enc_posture(&mut o, &b.posture);
    put_str(&mut o, &b.purpose.0);
    o.push(b.suite.as_u8());
    o.extend_from_slice(&b.issued_at.to_le_bytes());
    o.extend_from_slice(&b.ttl.to_le_bytes());
    o.extend_from_slice(b.rms.expose());
    SecretBytes::new(o)
}
fn decode_ticket_body(body: &[u8]) -> Result<TicketBody, TransportError> {
    let mut r = Reader::new(body);
    let cert = decode_cert(r.bytes()?)?;
    let posture = dec_posture(&mut r)?;
    let purpose = Purpose(r.string()?);
    let suite = r.suite()?;
    let issued_at = r.u64()?;
    let ttl = r.u64()?;
    let rms = SecretArray::new(r.arr32()?);
    r.finish()?;
    Ok(TicketBody {
        cert,
        posture,
        purpose,
        suite,
        issued_at,
        ttl,
        rms,
    })
}

/// The ticket AEAD's associated data: the version label and the key id, so a ticket
/// cannot be presented under another key's id.
fn ticket_aad(key_id: u8) -> Vec<u8> {
    let mut aad = TICKET_AAD.to_vec();
    aad.push(key_id);
    aad
}

// ---- ticket issuance ------------------------------------------------------

/// Issue a resumption ticket for a just-completed **responder** session. The
/// responder seals the returned bytes in a record under [`NEW_TICKET_AAD`] and sends
/// it to the initiator; it keeps no per-ticket state (the RMS lives inside the sealed
/// ticket). The ticket is `key id || nonce || AEAD(body)` and records the client's
/// full certificate and what the handshake authorised it for; its lifetime is `ttl`
/// seconds from `now`, cut short at the certificate's expiry. [MG-SEC-02]
/// [MG-SEC-04] [VLD-ANB-MIDGARD-0046]
pub fn issue_ticket(
    key: &TicketKey,
    session: &Session,
    now: u64,
    ttl: u64,
    rng: &mut dyn Rng,
) -> Result<Vec<u8>, MtlsError> {
    let not_responder = || MtlsError::Protocol("tickets are issued by the responder".into());
    let Authorization { clearance, purpose } =
        session.authorization().cloned().ok_or_else(not_responder)?;
    let cert = session
        .peer_certificate()
        .cloned()
        .ok_or_else(not_responder)?;
    let body = TicketBody {
        ttl: ttl.min(cert.terms.not_after.saturating_sub(now)),
        cert,
        posture: clearance,
        purpose,
        suite: session.suite(),
        issued_at: now,
        rms: session.resumption_secret(),
    };
    let plaintext = encode_ticket_body(&body);
    let mut nonce = [0u8; GCM_NONCE_LEN];
    rng.fill(&mut nonce);
    let sealed = key
        .aead()
        .seal(&nonce, &ticket_aad(key.id), plaintext.expose())
        .map_err(MtlsError::Crypto)?;
    let mut out = Vec::with_capacity(1 + GCM_NONCE_LEN + sealed.len());
    out.push(key.id);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Open a ticket with whichever of `keys` its first byte names. A ticket whose key
/// has been retired is refused like an expired one.
fn open_ticket(keys: &[&TicketKey], ticket: &[u8]) -> Result<TicketBody, MtlsError> {
    if ticket.len() < 1 + GCM_NONCE_LEN {
        return Err(MtlsError::Transport(TransportError::Malformed(
            "ticket too short".into(),
        )));
    }
    let key_id = ticket[0];
    let key = keys
        .iter()
        .find(|k| k.id == key_id)
        .ok_or(MtlsError::Authz(AuthzError::Denied))?;
    let (nonce, sealed) = ticket[1..].split_at(GCM_NONCE_LEN);
    let mut n = [0u8; GCM_NONCE_LEN];
    n.copy_from_slice(nonce);
    let plaintext = SecretBytes::new(
        key.aead()
            .open(&n, &ticket_aad(key_id), sealed)
            .map_err(MtlsError::Crypto)?,
    );
    decode_ticket_body(plaintext.expose()).map_err(MtlsError::Transport)
}

// ---- resumption wire messages ---------------------------------------------
//
// ResumeHello  = ticket || nonce_c || kem_pk || binder   (the binder covers the rest)
// ResumeAccept = nonce_s || ct || finished_r
//
// The ticket, KEM public key and ciphertext are length-prefixed and the key and
// ciphertext carry their suite byte, so each message has exactly one parse.

/// The part of a ResumeHello the binder covers: everything before the binder.
fn encode_resume_hello_prefix(ticket: &[u8], nonce_c: &[u8; 32], kem_pk: &KemPublicKey) -> Vec<u8> {
    let mut o = Vec::new();
    put_bytes(&mut o, ticket);
    o.extend_from_slice(nonce_c);
    o.push(kem_pk.suite().as_u8());
    put_bytes(&mut o, kem_pk.as_bytes());
    o
}

/// A decoded ResumeHello.
struct ResumeHello<'a> {
    ticket: &'a [u8],
    kem_pk: KemPublicKey,
    binder: [u8; 32],
    /// The bytes the binder covers.
    covered: &'a [u8],
}

fn decode_resume_hello(body: &[u8]) -> Result<ResumeHello<'_>, TransportError> {
    let mut r = Reader::new(body);
    let ticket = r.bytes()?;
    let _nonce_c = r.arr32()?;
    let suite = r.suite()?;
    let kem_pk = KemPublicKey::new(suite, r.bytes()?.to_vec());
    let covered_len = r.pos;
    let binder = r.arr32()?;
    r.finish()?;
    Ok(ResumeHello {
        ticket,
        kem_pk,
        binder,
        covered: &body[..covered_len],
    })
}

/// The part of a ResumeAccept folded into the key schedule: everything before the
/// finished MAC.
fn encode_resume_accept_prefix(nonce_s: &[u8; 32], ct: &Ciphertext) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend_from_slice(nonce_s);
    o.push(ct.suite().as_u8());
    put_bytes(&mut o, ct.as_bytes());
    o
}

/// A decoded ResumeAccept.
struct ResumeAccept<'a> {
    ct: Ciphertext,
    finished_r: [u8; 32],
    /// Everything before the finished MAC.
    prefix: &'a [u8],
}

fn decode_resume_accept(body: &[u8]) -> Result<ResumeAccept<'_>, TransportError> {
    let mut r = Reader::new(body);
    let _nonce_s = r.arr32()?;
    let suite = r.suite()?;
    let ct = Ciphertext::new(suite, r.bytes()?.to_vec());
    let prefix_len = r.pos;
    let finished_r = r.arr32()?;
    r.finish()?;
    Ok(ResumeAccept {
        ct,
        finished_r,
        prefix: &body[..prefix_len],
    })
}

// ---- key schedule ---------------------------------------------------------

fn binder_key(rms: &[u8; 32]) -> SecretArray<32> {
    SecretArray::new(hkdf_sha256_32(BINDER_INFO, rms, b"binder-key"))
}
/// The binder: a MAC under a key derived from the RMS over everything in the
/// ResumeHello before it, so it covers the ticket, the client nonce and the fresh
/// KEM share.
fn compute_binder(rms: &[u8; 32], covered: &[u8]) -> [u8; 32] {
    let bk = binder_key(rms);
    let mut msg = Vec::with_capacity(BINDER_LABEL.len() + covered.len());
    msg.extend_from_slice(BINDER_LABEL);
    msg.extend_from_slice(covered);
    hmac_sha256(bk.expose(), &msg)
}
/// The resumed traffic secret, with the RFC 5869 roles the right way round: the salt
/// is public (the hash of the ResumeHello and of the ResumeAccept up to its finished
/// MAC) and the input keying material secret (`RMS || KEM shared secret`).
/// [VLD-ANB-MIDGARD-0046]
fn resume_traffic(
    rms: &[u8; 32],
    ss: &[u8; 32],
    hello: &[u8],
    accept_prefix: &[u8],
) -> SecretArray<32> {
    let mut transcript = Vec::with_capacity(hello.len() + accept_prefix.len());
    transcript.extend_from_slice(hello);
    transcript.extend_from_slice(accept_prefix);
    let salt = sha256(&transcript);
    let mut ikm = SecretArray::<64>::zeroed();
    ikm.expose_mut()[..32].copy_from_slice(rms);
    ikm.expose_mut()[32..].copy_from_slice(ss);
    SecretArray::new(hkdf_sha256_32(&salt, ikm.expose(), RESUME_INFO))
}
fn server_finished(traffic: &[u8; 32]) -> [u8; 32] {
    hmac_sha256(traffic, SERVER_FINISHED_LABEL)
}

// ---- drivers over a real stream -------------------------------------------

/// Refuse to resume with a server whose certificate has since been revoked or has
/// expired. [MG-SEC-02] [MG-SEC-03]
fn recheck_peer_cert(anchor: &TrustAnchor, cert: PeerCert, now: u64) -> Result<(), MtlsError> {
    if anchor.is_revoked(&cert.fingerprint) {
        return Err(MtlsError::PeerCertRevoked);
    }
    if now >= cert.not_after {
        return Err(MtlsError::PeerCertExpired {
            not_before: 0,
            not_after: cert.not_after,
            now,
        });
    }
    Ok(())
}

/// Drive the initiator side of a resumption over `stream` (already carrying the
/// [`MODE_RESUME`] byte written by the caller). Before sending anything it re-checks
/// the server's certificate against `anchor`'s current revocation state and its
/// expiry at `now`. It draws the client nonce and then an ephemeral KEM key pair in
/// the ticket's suite from `rng`, sends them under the binder, and derives the
/// traffic secret from the responder's encapsulation. Returns the resumed session.
pub fn run_resume_initiator<S: std::io::Read + std::io::Write>(
    stream: &mut S,
    ticket: &ClientTicket,
    anchor: &TrustAnchor,
    provider: &CryptoProvider,
    now: u64,
    rng: &mut dyn Rng,
) -> Result<Session, MtlsError> {
    recheck_peer_cert(anchor, ticket.server_cert, now)?;
    let kem = provider
        .kem_for(ticket.suite)
        .map_err(|_| MtlsError::BackendUnavailable)?;
    let mut nonce_c = [0u8; 32];
    rng.fill(&mut nonce_c);
    let (kem_pk, kem_sk) = kem.keypair(rng).map_err(MtlsError::Crypto)?;
    let mut hello = encode_resume_hello_prefix(&ticket.ticket, &nonce_c, &kem_pk);
    let binder = compute_binder(ticket.rms.expose(), &hello);
    hello.extend_from_slice(&binder);
    write_frame(stream, &hello).map_err(MtlsError::Transport)?;

    let accept = read_handshake_frame(stream).map_err(MtlsError::Transport)?;
    let accept = decode_resume_accept(&accept).map_err(MtlsError::Transport)?;
    if accept.ct.suite() != ticket.suite {
        return Err(MtlsError::Protocol(
            "resumption ciphertext is not in the ticket's suite".into(),
        ));
    }
    let ss = kem
        .decapsulate(&kem_sk, &accept.ct)
        .map_err(MtlsError::Crypto)?;
    let traffic = resume_traffic(ticket.rms.expose(), ss.expose(), &hello, accept.prefix);
    if !ct_eq(&server_finished(traffic.expose()), &accept.finished_r) {
        return Err(MtlsError::KeyConfirmationFailed);
    }
    Ok(Session::new(
        ticket.server.clone(),
        ticket.server_cert,
        None,
        ticket.suite,
        traffic,
    ))
}

/// The outcome of accepting a resumption: the resumed session plus the client
/// identity/posture/purpose recovered from the ticket (for re-authorization and
/// per-identity accounting).
pub struct Resumed {
    /// The resumed, authenticated session.
    pub session: Session,
    /// The authenticated client identity (from the ticket).
    pub client: WorkloadIdentity,
}

/// Drive the responder side of a resumption over `stream` (the [`MODE_RESUME`] byte
/// has already been consumed by the caller). The checks run in a fixed order: open
/// the ticket with whichever of `keys` it names, its TTL, the initiator's binder
/// (client authentication), the replay marker, the client certificate re-verified
/// against `anchor` (CA signature, validity window at `now`, current revocation
/// list), and the PDP. Only then does it encapsulate to the initiator's KEM share and
/// return the resumed session. `check_replay(ticket, expires_at)` returns `true` if
/// the ticket is fresh (unseen) and remembers it until `expires_at` — supply a real
/// single-use guard in production. A failed binder never reaches it, so an
/// unauthenticated ResumeHello cannot consume a ticket. [VLD-ANB-MIDGARD-0046]
#[allow(clippy::too_many_arguments)]
pub fn run_resume_responder<S, F>(
    stream: &mut S,
    keys: &[&TicketKey],
    anchor: &TrustAnchor,
    provider: &CryptoProvider,
    pdp: &dyn PolicyDecisionPoint,
    resource: &Resource,
    now: u64,
    check_replay: F,
    rng: &mut dyn Rng,
) -> Result<Resumed, MtlsError>
where
    S: std::io::Read + std::io::Write,
    F: FnOnce(&[u8], u64) -> bool,
{
    let hello = read_handshake_frame(stream).map_err(MtlsError::Transport)?;
    let parsed = decode_resume_hello(&hello).map_err(MtlsError::Transport)?;

    // 1. The ticket opens under one of our keys.
    let body = open_ticket(keys, parsed.ticket)?;
    // 2. TTL: an expired ticket forces a fresh full handshake.
    let expires_at = body.issued_at.saturating_add(body.ttl);
    if now > expires_at {
        return Err(MtlsError::Authz(AuthzError::Denied));
    }
    // 3. The binder proves the initiator holds the RMS and binds its KEM share. It is
    //    checked before the replay marker, so a forged hello cannot burn the ticket.
    let expected = compute_binder(body.rms.expose(), parsed.covered);
    if !ct_eq(&expected, &parsed.binder) {
        return Err(MtlsError::KeyConfirmationFailed);
    }
    if parsed.kem_pk.suite() != body.suite {
        return Err(MtlsError::Protocol(
            "resumption key share is not in the ticket's suite".into(),
        ));
    }
    // 4. Single-use: reject a replayed ticket.
    if !check_replay(parsed.ticket, expires_at) {
        return Err(MtlsError::Authz(AuthzError::Denied));
    }
    // 5. The client's certificate must still verify: CA signature, validity window,
    //    and the revocation list in force now. [MG-SEC-02] [MG-SEC-03]
    verify_cert(&body.cert, anchor, provider, now)?;
    // 6. Re-authorize the channel exactly as a full handshake would.
    authorize(
        pdp,
        &body.cert.identity,
        &body.purpose,
        &body.posture,
        resource,
    )?;

    // Fresh key exchange: encapsulate to the initiator's ephemeral share.
    let kem = provider
        .kem_for(body.suite)
        .map_err(|_| MtlsError::BackendUnavailable)?;
    let mut nonce_s = [0u8; 32];
    rng.fill(&mut nonce_s);
    let (ct, ss) = kem
        .encapsulate(&parsed.kem_pk, rng)
        .map_err(MtlsError::Crypto)?;
    let mut accept = encode_resume_accept_prefix(&nonce_s, &ct);
    let traffic = resume_traffic(body.rms.expose(), ss.expose(), &hello, &accept);
    accept.extend_from_slice(&server_finished(traffic.expose()));
    write_frame(stream, &accept).map_err(MtlsError::Transport)?;

    let client = body.cert.identity.clone();
    let client_cert = PeerCert {
        fingerprint: body.cert.fingerprint(),
        not_after: body.cert.terms.not_after,
    };
    let session = Session::new(
        client.clone(),
        client_cert,
        Some(Authorization {
            clearance: body.posture,
            purpose: body.purpose,
        }),
        body.suite,
        traffic,
    )
    .with_peer_certificate(body.cert);
    Ok(Resumed { session, client })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{certify, CertTerms, ServiceId};
    use policy::{
        Action, DefaultPdp, Effect, Marking, PolicySet, ResourceId, Role, Rule, RuleId, Selector,
    };
    use pqc_agility::backend::fips::provider as fips_provider;
    use pqc_agility::{SigId, SigningKey};

    const NOT_AFTER: u64 = 10_000;

    struct SeedRng(u64);
    impl pqc_agility::Rng for SeedRng {
        fn fill(&mut self, buf: &mut [u8]) {
            let mut i = 0;
            while i < buf.len() {
                self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = self.0;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                z ^= z >> 31;
                let b = z.to_le_bytes();
                let take = (buf.len() - i).min(8);
                buf[i..i + take].copy_from_slice(&b[..take]);
                i += take;
            }
        }
    }

    fn identity(name: &str, role: &str, seed: u64) -> WorkloadIdentity {
        let (vk, _sk) = fips_provider()
            .sig_for(SigId::MlDsa65)
            .unwrap()
            .keypair(&mut SeedRng(seed))
            .unwrap();
        WorkloadIdentity {
            id: ServiceId(name.into()),
            roles: [Role(role.into())].into_iter().collect(),
            sig_alg: SigId::MlDsa65,
            verifying_key: vk,
        }
    }

    fn client_identity() -> WorkloadIdentity {
        identity("svc-client", "analyst", 7)
    }

    fn server_identity() -> WorkloadIdentity {
        identity("svc-gateway", "server", 9)
    }

    fn cert_of(id: &WorkloadIdentity) -> PeerCert {
        PeerCert {
            fingerprint: pqc_agility::sha256(id.verifying_key.as_bytes()),
            not_after: NOT_AFTER,
        }
    }

    fn ca_key() -> (TrustAnchor, SigningKey) {
        let (vk, sk) = fips_provider()
            .sig_for(SigId::MlDsa65)
            .unwrap()
            .keypair(&mut SeedRng(99))
            .unwrap();
        (TrustAnchor::new(SigId::MlDsa65, vk), sk)
    }

    /// A fresh anchor for the test CA (each call has its own revocation state).
    fn anchor() -> TrustAnchor {
        ca_key().0
    }

    fn client_cert() -> IdentityCert {
        let (_, sk) = ca_key();
        certify(
            &fips_provider(),
            SigId::MlDsa65,
            &sk,
            &client_identity(),
            CertTerms::new(1, 0, NOT_AFTER, posture()),
        )
        .unwrap()
    }

    fn resource() -> Resource {
        Resource {
            id: ResourceId("channel".into()),
            marking: Marking {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
        }
    }

    fn permit_pdp() -> DefaultPdp {
        DefaultPdp::new(
            PolicySet::new(vec![Rule {
                id: RuleId("r".into()),
                role: Role("analyst".into()),
                action: Action("OpenChannel".into()),
                purpose: None,
                selector: Selector::Any,
                effect: Effect::Permit,
            }])
            .unwrap(),
        )
    }

    fn posture() -> Posture {
        Posture {
            level: ClassificationLevel::SECRET,
            compartments: BTreeSet::new(),
        }
    }

    /// The two ends of a completed session (built directly for the resumption unit
    /// test — the full handshake is covered by the gateway integration tests).
    fn sessions() -> (Session, Session) {
        let cert = client_cert();
        let responder = Session::new(
            client_identity(),
            cert_of(&client_identity()),
            Some(Authorization {
                clearance: posture(),
                purpose: Purpose("investigation".into()),
            }),
            KemSuiteId::HybridX25519MlKem768,
            SecretArray::new([0x42; 32]),
        )
        .with_peer_certificate(cert);
        let initiator = Session::new(
            server_identity(),
            cert_of(&server_identity()),
            None,
            KemSuiteId::HybridX25519MlKem768,
            SecretArray::new([0x42; 32]),
        );
        (initiator, responder)
    }

    /// Resume over a real socket; returns both outcomes.
    fn resume(
        ticket: &ClientTicket,
        keys: &[&TicketKey],
        client_anchor: &TrustAnchor,
        server_anchor: &TrustAnchor,
        now: u64,
    ) -> (Result<Session, MtlsError>, Result<Resumed, MtlsError>) {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let pdp = permit_pdp();
        let res = resource();
        let provider = fips_provider();
        std::thread::scope(|s| {
            let server = s.spawn(|| {
                let (mut sock, _) = listener.accept().unwrap();
                let mut b = [0u8; 1];
                if sock.read_exact(&mut b).is_err() {
                    return Err(MtlsError::Transport(TransportError::Closed));
                }
                assert_eq!(b[0], MODE_RESUME);
                run_resume_responder(
                    &mut sock,
                    keys,
                    server_anchor,
                    &provider,
                    &pdp,
                    &res,
                    now,
                    |_t, _e| true,
                    &mut SeedRng(2),
                )
            });
            let mut client = TcpStream::connect(addr).unwrap();
            let cs = match client.write_all(&[MODE_RESUME]) {
                Ok(()) => run_resume_initiator(
                    &mut client,
                    ticket,
                    client_anchor,
                    &provider,
                    now,
                    &mut SeedRng(3),
                ),
                Err(e) => Err(MtlsError::Transport(TransportError::Io(e.to_string()))),
            };
            drop(client);
            (cs, server.join().unwrap())
        })
    }

    fn issued(key: &TicketKey, now: u64, ttl: u64) -> ClientTicket {
        let (initiator, responder) = sessions();
        let bytes = issue_ticket(key, &responder, now, ttl, &mut SeedRng(1)).unwrap();
        ClientTicket::new(&initiator, bytes)
    }

    #[test]
    fn round_trips_a_resumption_over_a_pipe() {
        let key = TicketKey::generate(&mut SeedRng(1));
        let ticket = issued(&key, 1_000, 3_600);
        let (cs, ss) = resume(&ticket, &[&key], &anchor(), &anchor(), 2_000);
        let (cs, resumed) = (cs.unwrap(), ss.unwrap());
        // Both sides derived the SAME traffic secret (resumption succeeded) …
        assert_eq!(cs.traffic_secret(), resumed.session.traffic_secret());
        // … the server recovered the authenticated client identity and what it was
        // authorised for …
        assert_eq!(resumed.client.id, ServiceId("svc-client".into()));
        assert_eq!(
            resumed.session.authorization().unwrap().clearance,
            posture()
        );
        // … and the client sees the server identity.
        assert_eq!(cs.peer().id, ServiceId("svc-gateway".into()));
    }

    #[test]
    fn a_ticket_never_outlives_the_client_certificate() {
        let key = TicketKey::generate(&mut SeedRng(1));
        let (_, responder) = sessions();
        let bytes =
            issue_ticket(&key, &responder, NOT_AFTER - 100, 3_600, &mut SeedRng(4)).unwrap();
        let body = open_ticket(&[&key], &bytes).unwrap();
        assert_eq!(body.ttl, 100, "TTL is cut at the certificate expiry");
    }

    /// MG-SEC-03: a client revoked after its ticket was issued cannot resume.
    #[test]
    fn a_revoked_client_cannot_resume() {
        let key = TicketKey::generate(&mut SeedRng(1));
        let ticket = issued(&key, 1_000, 3_600);
        let server_anchor = anchor();
        server_anchor.revoke(cert_of(&client_identity()).fingerprint);
        let (_, ss) = resume(&ticket, &[&key], &anchor(), &server_anchor, 2_000);
        assert!(matches!(ss, Err(MtlsError::PeerCertRevoked)));
    }

    /// VLD-ANB-MIDGARD-0046: the ticket's certificate is re-verified under the
    /// anchor, so a ticket for a certificate another CA signed does not resume.
    #[test]
    fn a_certificate_from_another_ca_cannot_resume() {
        let key = TicketKey::generate(&mut SeedRng(1));
        let ticket = issued(&key, 1_000, 3_600);
        let (vk, _) = fips_provider()
            .sig_for(SigId::MlDsa65)
            .unwrap()
            .keypair(&mut SeedRng(1234))
            .unwrap();
        let other_ca = TrustAnchor::new(SigId::MlDsa65, vk);
        let (_, ss) = resume(&ticket, &[&key], &anchor(), &other_ca, 2_000);
        assert!(matches!(ss, Err(MtlsError::PeerCertUntrusted)));
    }

    /// MG-SEC-03: a client does not resume with a server revoked since, and sends
    /// nothing to it.
    #[test]
    fn a_client_will_not_resume_with_a_revoked_server() {
        let key = TicketKey::generate(&mut SeedRng(1));
        let ticket = issued(&key, 1_000, 3_600);
        let client_anchor = anchor();
        client_anchor.revoke(cert_of(&server_identity()).fingerprint);
        let (cs, _) = resume(&ticket, &[&key], &client_anchor, &anchor(), 2_000);
        assert!(matches!(cs, Err(MtlsError::PeerCertRevoked)));
    }

    /// MG-SEC-02: past the certificate expiry the ticket is dead.
    #[test]
    fn an_expired_certificate_cannot_resume() {
        let key = TicketKey::generate(&mut SeedRng(1));
        let ticket = issued(&key, NOT_AFTER - 10, u64::MAX);
        let (_, ss) = resume(&ticket, &[&key], &anchor(), &anchor(), NOT_AFTER - 5);
        assert!(ss.is_ok());
        let ticket = issued(&key, NOT_AFTER - 10, u64::MAX);
        let (cs, _) = resume(&ticket, &[&key], &anchor(), &anchor(), NOT_AFTER + 1);
        assert!(matches!(cs, Err(MtlsError::PeerCertExpired { .. })));
    }

    #[test]
    fn a_tampered_ticket_fails_to_open() {
        let key = TicketKey::generate(&mut SeedRng(5));
        let (_, responder) = sessions();
        let mut ticket = issue_ticket(&key, &responder, 1_000, 3_600, &mut SeedRng(6)).unwrap();
        let last = ticket.len() - 1;
        ticket[last] ^= 0x01;
        assert!(
            open_ticket(&[&key], &ticket).is_err(),
            "AEAD tag must reject tamper"
        );
    }

    /// VLD-ANB-MIDGARD-0046: a ticket names its key; a retired key's tickets are
    /// refused, and relabelling a ticket with another key's id breaks its AEAD.
    #[test]
    fn a_ticket_opens_only_under_the_key_it_names() {
        let (_, responder) = sessions();
        let current = TicketKey::new([7; 32]).with_id(1);
        let ticket = issue_ticket(&current, &responder, 1_000, 3_600, &mut SeedRng(6)).unwrap();
        assert_eq!(ticket[0], 1, "the first byte is the key id");

        let previous = TicketKey::new([8; 32]);
        assert!(open_ticket(&[&previous, &current], &ticket).is_ok());
        assert!(matches!(
            open_ticket(&[&previous], &ticket),
            Err(MtlsError::Authz(AuthzError::Denied))
        ));

        let same_bytes_other_id = TicketKey::new([7; 32]).with_id(2);
        let mut relabelled = ticket.clone();
        relabelled[0] = 2;
        assert!(matches!(
            open_ticket(&[&same_bytes_other_id], &relabelled),
            Err(MtlsError::Crypto(_))
        ));
    }

    #[test]
    fn only_the_responder_issues_tickets() {
        let key = TicketKey::generate(&mut SeedRng(5));
        let (initiator, _) = sessions();
        assert!(issue_ticket(&key, &initiator, 1_000, 3_600, &mut SeedRng(6)).is_err());
    }
}
