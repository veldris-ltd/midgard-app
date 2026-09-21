//! Abbreviated (session-resumption) handshake — TLS 1.3-style PSK resumption for [NFR-SEC-1]
//! PQ-mTLS. After a full mutual handshake, the responder issues the initiator an
//! opaque **session ticket**: the resumption-master secret (RMS) plus the peer's
//! authenticated identity, posture, and purpose, AEAD-sealed under a responder-held
//! ticket key. A later connection can then *resume* without re-running the KEM
//! encapsulation or the ML-DSA signature flights — the expensive part of the
//! handshake is amortised. [RT-25]
//!
//! Security model (mirrors TLS 1.3 PSK-without-DHE):
//! - The initiator proves possession of the RMS with a **binder** MAC. Only a party
//!   that completed the original mutually-authenticated session knows the RMS, so a
//!   valid binder authenticates the client — no signature needed.
//! - The responder proves possession of both the ticket key (it opened the ticket)
//!   and the RMS with its **finished** MAC — authenticating the server to the client.
//! - Fresh nonces on each side derive a fresh per-resumption traffic secret, so
//!   resumed sessions never share record keys.
//! - Tickets are time-boxed (a TTL the responder enforces) and single-use when the
//!   caller supplies a replay guard (see the gateway's session manager).
//!
//! Forward secrecy is reduced on a resumed leg (no fresh KEM), the standard PSK
//! trade-off; the responder bounds exposure with a short ticket TTL and re-runs a
//! full handshake once the ticket expires.
//!
//! Requires the production AEAD, so this module is gated on `backend-fips`.

use std::collections::BTreeSet;

use policy::{ClassificationLevel, Compartment, PolicyDecisionPoint, Purpose, Resource, Role};
use pqc_agility::hash::{ct_eq, hkdf_sha256_32, hmac_sha256};
use pqc_agility::{Aes256GcmAead, KemSuiteId, Rng, SigId, VerifyingKey, GCM_NONCE_LEN};

use crate::error::{MtlsError, TransportError};
use crate::handshake::authorize;
use crate::identity::{ServiceId, WorkloadIdentity};
use crate::posture::Posture;
use crate::session::Session;
use crate::wire::{read_frame, write_frame};

const TICKET_AAD: &[u8] = b"midgard-pqmtls-ticket-v1";
const RESUME_INFO: &[u8] = b"midgard-pqmtls-resume-traffic-v1";
const BINDER_INFO: &[u8] = b"midgard-pqmtls-resume-binder-v1";
const BINDER_LABEL: &[u8] = b"resume-binder";
const SERVER_FINISHED_LABEL: &[u8] = b"resume-server-finished";

/// Tag for the very first frame on a connection, so the responder knows whether a
/// full handshake or a resumption follows. [RT-25]
pub const MODE_FULL: u8 = 0x01;
/// Resumption connection mode (see [`MODE_FULL`]).
pub const MODE_RESUME: u8 = 0x02;

/// A responder-held secret that seals/opens session tickets. It never leaves the
/// responder; the initiator only ever sees opaque sealed bytes.
#[derive(Clone)]
pub struct TicketKey([u8; 32]);

impl TicketKey {
    /// Wrap raw key bytes (e.g. from the KMS / a rotated ticket-key schedule).
    pub fn new(key: [u8; 32]) -> Self {
        Self(key)
    }

    /// Generate a fresh random ticket key.
    pub fn generate(rng: &mut dyn Rng) -> Self {
        let mut k = [0u8; 32];
        rng.fill(&mut k);
        Self(k)
    }

    fn aead(&self) -> Aes256GcmAead {
        Aes256GcmAead::new(&self.0)
    }
}

/// What the initiator keeps after a full handshake so it can resume later: the
/// opaque ticket, the RMS (kept locally, never on the wire), and the authenticated
/// server identity + suite to reconstruct the resumed [`Session`].
#[derive(Clone)]
pub struct ClientTicket {
    /// The opaque, responder-sealed ticket bytes.
    pub ticket: Vec<u8>,
    rms: [u8; 32],
    server: WorkloadIdentity,
    suite: KemSuiteId,
}

impl ClientTicket {
    /// Build the initiator-side resumption state from a completed session and the
    /// ticket bytes the responder sent.
    pub fn new(session: &Session, ticket: Vec<u8>) -> Self {
        Self {
            ticket,
            rms: session.resumption_secret(),
            server: session.peer().clone(),
            suite: session.suite(),
        }
    }
}

// ---- ticket contents (responder-private plaintext) ------------------------

/// The sealed contents of a ticket. Never exposed to the initiator.
struct TicketBody {
    client: WorkloadIdentity,
    posture: Posture,
    purpose: Purpose,
    suite: KemSuiteId,
    issued_at: u64,
    ttl: u64,
    rms: [u8; 32],
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
        String::from_utf8(self.bytes()?).map_err(|_| TransportError::Malformed("utf-8".into()))
    }
    fn sig_id(&mut self) -> Result<SigId, TransportError> {
        SigId::from_u8(self.u8()?).ok_or_else(|| TransportError::Malformed("sig id".into()))
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

fn enc_identity(o: &mut Vec<u8>, id: &WorkloadIdentity) {
    put_str(o, &id.id.0);
    put_u32(o, id.roles.len() as u32);
    for r in &id.roles {
        put_str(o, &r.0);
    }
    o.push(id.sig_alg.as_u8());
    o.push(id.verifying_key.alg().as_u8());
    put_bytes(o, id.verifying_key.as_bytes());
}
fn dec_identity(r: &mut Reader) -> Result<WorkloadIdentity, TransportError> {
    let id = ServiceId(r.string()?);
    let n = r.u32()? as usize;
    let mut roles = BTreeSet::new();
    for _ in 0..n {
        roles.insert(Role(r.string()?));
    }
    let sig_alg = r.sig_id()?;
    let vk_alg = r.sig_id()?;
    let vk = VerifyingKey::new(vk_alg, r.bytes()?);
    Ok(WorkloadIdentity {
        id,
        roles,
        sig_alg,
        verifying_key: vk,
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

fn encode_ticket_body(b: &TicketBody) -> Vec<u8> {
    let mut o = Vec::new();
    enc_identity(&mut o, &b.client);
    enc_posture(&mut o, &b.posture);
    put_str(&mut o, &b.purpose.0);
    o.push(b.suite.as_u8());
    o.extend_from_slice(&b.issued_at.to_le_bytes());
    o.extend_from_slice(&b.ttl.to_le_bytes());
    o.extend_from_slice(&b.rms);
    o
}
fn decode_ticket_body(body: &[u8]) -> Result<TicketBody, TransportError> {
    let mut r = Reader::new(body);
    let client = dec_identity(&mut r)?;
    let posture = dec_posture(&mut r)?;
    let purpose = Purpose(r.string()?);
    let suite = r.suite()?;
    let issued_at = r.u64()?;
    let ttl = r.u64()?;
    let rms = r.arr32()?;
    r.finish()?;
    Ok(TicketBody {
        client,
        posture,
        purpose,
        suite,
        issued_at,
        ttl,
        rms,
    })
}

// ---- ticket issuance ------------------------------------------------------

/// Issue a resumption ticket for a just-completed session. The responder calls
/// this and sends the returned bytes to the initiator; it keeps no per-ticket
/// state (the RMS lives inside the sealed ticket). `now`/`ttl` are seconds.
#[allow(clippy::too_many_arguments)]
pub fn issue_ticket(
    key: &TicketKey,
    session: &Session,
    client: &WorkloadIdentity,
    posture: &Posture,
    purpose: &Purpose,
    now: u64,
    ttl: u64,
    rng: &mut dyn Rng,
) -> Result<Vec<u8>, MtlsError> {
    let body = TicketBody {
        client: client.clone(),
        posture: posture.clone(),
        purpose: purpose.clone(),
        suite: session.suite(),
        issued_at: now,
        ttl,
        rms: session.resumption_secret(),
    };
    let plaintext = encode_ticket_body(&body);
    let mut nonce = [0u8; GCM_NONCE_LEN];
    rng.fill(&mut nonce);
    let sealed = key
        .aead()
        .seal(&nonce, TICKET_AAD, &plaintext)
        .map_err(MtlsError::Crypto)?;
    let mut out = Vec::with_capacity(GCM_NONCE_LEN + sealed.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

fn open_ticket(key: &TicketKey, ticket: &[u8]) -> Result<TicketBody, MtlsError> {
    if ticket.len() < GCM_NONCE_LEN {
        return Err(MtlsError::Transport(TransportError::Malformed(
            "ticket too short".into(),
        )));
    }
    let (nonce, sealed) = ticket.split_at(GCM_NONCE_LEN);
    let mut n = [0u8; GCM_NONCE_LEN];
    n.copy_from_slice(nonce);
    let plaintext = key
        .aead()
        .open(&n, TICKET_AAD, sealed)
        .map_err(MtlsError::Crypto)?;
    decode_ticket_body(&plaintext).map_err(MtlsError::Transport)
}

// ---- resumption wire messages ---------------------------------------------

fn encode_resume_hello(ticket: &[u8], nonce_c: &[u8; 32], binder: &[u8; 32]) -> Vec<u8> {
    let mut o = Vec::new();
    put_bytes(&mut o, ticket);
    o.extend_from_slice(nonce_c);
    o.extend_from_slice(binder);
    o
}
/// A decoded ResumeHello: the session ticket, the client nonce, and the binder
/// proving the client holds the resumption secret.
type ResumeHello = (Vec<u8>, [u8; 32], [u8; 32]);

fn decode_resume_hello(body: &[u8]) -> Result<ResumeHello, TransportError> {
    let mut r = Reader::new(body);
    let ticket = r.bytes()?;
    let nonce_c = r.arr32()?;
    let binder = r.arr32()?;
    r.finish()?;
    Ok((ticket, nonce_c, binder))
}

fn encode_resume_accept(nonce_s: &[u8; 32], finished_r: &[u8; 32]) -> Vec<u8> {
    let mut o = Vec::with_capacity(64);
    o.extend_from_slice(nonce_s);
    o.extend_from_slice(finished_r);
    o
}
fn decode_resume_accept(body: &[u8]) -> Result<([u8; 32], [u8; 32]), TransportError> {
    let mut r = Reader::new(body);
    let nonce_s = r.arr32()?;
    let finished_r = r.arr32()?;
    r.finish()?;
    Ok((nonce_s, finished_r))
}

// ---- key schedule ---------------------------------------------------------

fn binder_key(rms: &[u8; 32]) -> [u8; 32] {
    hkdf_sha256_32(BINDER_INFO, rms, b"binder-key")
}
fn compute_binder(rms: &[u8; 32], ticket: &[u8], nonce_c: &[u8; 32]) -> [u8; 32] {
    let bk = binder_key(rms);
    let mut msg = Vec::with_capacity(BINDER_LABEL.len() + ticket.len() + 32);
    msg.extend_from_slice(BINDER_LABEL);
    msg.extend_from_slice(ticket);
    msg.extend_from_slice(nonce_c);
    hmac_sha256(&bk, &msg)
}
fn resume_traffic(rms: &[u8; 32], nonce_c: &[u8; 32], nonce_s: &[u8; 32]) -> [u8; 32] {
    let mut ikm = [0u8; 64];
    ikm[..32].copy_from_slice(nonce_c);
    ikm[32..].copy_from_slice(nonce_s);
    hkdf_sha256_32(rms, &ikm, RESUME_INFO)
}
fn server_finished(traffic: &[u8; 32]) -> [u8; 32] {
    hmac_sha256(traffic, SERVER_FINISHED_LABEL)
}

// ---- drivers over a real stream -------------------------------------------

/// Drive the initiator side of a resumption over `stream` (already carrying the
/// [`MODE_RESUME`] byte written by the caller). Returns the resumed session.
pub fn run_resume_initiator<S: std::io::Read + std::io::Write>(
    stream: &mut S,
    ticket: &ClientTicket,
    rng: &mut dyn Rng,
) -> Result<Session, MtlsError> {
    let mut nonce_c = [0u8; 32];
    rng.fill(&mut nonce_c);
    let binder = compute_binder(&ticket.rms, &ticket.ticket, &nonce_c);
    let hello = encode_resume_hello(&ticket.ticket, &nonce_c, &binder);
    write_frame(stream, &hello).map_err(MtlsError::Transport)?;

    let accept = read_frame(stream).map_err(MtlsError::Transport)?;
    let (nonce_s, finished_r) = decode_resume_accept(&accept).map_err(MtlsError::Transport)?;

    let traffic = resume_traffic(&ticket.rms, &nonce_c, &nonce_s);
    if !ct_eq(&server_finished(&traffic), &finished_r) {
        return Err(MtlsError::KeyConfirmationFailed);
    }
    Ok(Session::new(ticket.server.clone(), ticket.suite, traffic))
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
/// has already been consumed by the caller). Re-checks the ticket TTL, verifies the
/// initiator's binder (client authentication), re-authorizes the channel against
/// the PDP, and returns the resumed session. `check_replay` returns `true` if the
/// ticket is fresh (unseen) — supply a real single-use guard in production.
#[allow(clippy::too_many_arguments)]
pub fn run_resume_responder<S, F>(
    stream: &mut S,
    key: &TicketKey,
    pdp: &dyn PolicyDecisionPoint,
    resource: &Resource,
    now: u64,
    check_replay: F,
    rng: &mut dyn Rng,
) -> Result<Resumed, MtlsError>
where
    S: std::io::Read + std::io::Write,
    F: FnOnce(&[u8]) -> bool,
{
    let hello = read_frame(stream).map_err(MtlsError::Transport)?;
    let (ticket, nonce_c, binder) = decode_resume_hello(&hello).map_err(MtlsError::Transport)?;

    let body = open_ticket(key, &ticket)?;

    // TTL check — an expired ticket forces a fresh full handshake.
    if now > body.issued_at.saturating_add(body.ttl) {
        return Err(MtlsError::Authz(crate::error::AuthzError::Denied));
    }
    // Single-use: reject a replayed ticket.
    if !check_replay(&ticket) {
        return Err(MtlsError::Authz(crate::error::AuthzError::Denied));
    }
    // Binder proves the initiator holds the RMS → authenticates the client.
    let expected = compute_binder(&body.rms, &ticket, &nonce_c);
    if !ct_eq(&expected, &binder) {
        return Err(MtlsError::KeyConfirmationFailed);
    }
    // Re-authorize the channel exactly as a full handshake would.
    authorize(pdp, &body.client, &body.purpose, &body.posture, resource)?;

    let mut nonce_s = [0u8; 32];
    rng.fill(&mut nonce_s);
    let traffic = resume_traffic(&body.rms, &nonce_c, &nonce_s);
    let finished_r = server_finished(&traffic);
    write_frame(stream, &encode_resume_accept(&nonce_s, &finished_r))
        .map_err(MtlsError::Transport)?;

    let session = Session::new(body.client.clone(), body.suite, traffic);
    Ok(Resumed {
        session,
        client: body.client,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::ServiceId;
    use policy::{
        Action, DefaultPdp, Effect, Marking, PolicySet, ResourceId, Rule, RuleId, Selector,
    };
    use pqc_agility::backend::fips::provider as fips_provider;
    use pqc_agility::SigId;

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

    fn client_identity() -> WorkloadIdentity {
        let (vk, _sk) = fips_provider()
            .sig_for(SigId::MlDsa65)
            .unwrap()
            .keypair(&mut SeedRng(7))
            .unwrap();
        WorkloadIdentity {
            id: ServiceId("svc-client".into()),
            roles: [Role("analyst".into())].into_iter().collect(),
            sig_alg: SigId::MlDsa65,
            verifying_key: vk,
        }
    }

    fn server_identity() -> WorkloadIdentity {
        let (vk, _sk) = fips_provider()
            .sig_for(SigId::MlDsa65)
            .unwrap()
            .keypair(&mut SeedRng(9))
            .unwrap();
        WorkloadIdentity {
            id: ServiceId("svc-gateway".into()),
            roles: [Role("server".into())].into_iter().collect(),
            sig_alg: SigId::MlDsa65,
            verifying_key: vk,
        }
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

    /// A completed session (built directly for the resumption unit test — the full
    /// handshake is covered by the gateway integration tests).
    fn a_session() -> Session {
        Session::new(
            server_identity(),
            KemSuiteId::HybridX25519MlKem768,
            [0x42; 32],
        )
    }

    #[test]
    fn round_trips_a_resumption_over_a_pipe() {
        let mut rng = SeedRng(1);
        let key = TicketKey::generate(&mut rng);
        let full = a_session();
        // Server issues a ticket for the client; client stores it.
        let ticket_bytes = issue_ticket(
            &key,
            &full,
            &client_identity(),
            &posture(),
            &Purpose("investigation".into()),
            1_000,
            3_600,
            &mut rng,
        )
        .unwrap();
        // On the initiator side, the RMS/server come from ITS copy of the session.
        let client_ticket = ClientTicket {
            ticket: ticket_bytes,
            rms: full.resumption_secret(),
            server: server_identity(),
            suite: KemSuiteId::HybridX25519MlKem768,
        };

        // A byte pipe: initiator writes, responder reads, and vice versa. We run
        // the responder first (it blocks on read), so use two threads.
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let pdp = permit_pdp();
        let res = resource();
        std::thread::scope(|s| {
            let server = s.spawn(|| {
                let (mut sock, _) = listener.accept().unwrap();
                let mode = {
                    let mut b = [0u8; 1];
                    sock.read_exact(&mut b).unwrap();
                    b[0]
                };
                assert_eq!(mode, MODE_RESUME);
                run_resume_responder(
                    &mut sock,
                    &key,
                    &pdp,
                    &res,
                    2_000,
                    |_t| true,
                    &mut SeedRng(2),
                )
            });

            let mut client = TcpStream::connect(addr).unwrap();
            client.write_all(&[MODE_RESUME]).unwrap();
            let cs = run_resume_initiator(&mut client, &client_ticket, &mut SeedRng(3)).unwrap();

            let resumed = server.join().unwrap().unwrap();
            // Both sides derived the SAME traffic secret (resumption succeeded) …
            assert_eq!(cs.traffic_secret(), resumed.session.traffic_secret());
            // … the server recovered the authenticated client identity …
            assert_eq!(resumed.client.id, ServiceId("svc-client".into()));
            // … and the client sees the server identity.
            assert_eq!(cs.peer().id, ServiceId("svc-gateway".into()));
        });
    }

    #[test]
    fn expired_ticket_is_refused() {
        let mut rng = SeedRng(1);
        let key = TicketKey::generate(&mut rng);
        let body = open_ticket(
            &key,
            &issue_ticket(
                &key,
                &a_session(),
                &client_identity(),
                &posture(),
                &Purpose("investigation".into()),
                1_000,
                100,
                &mut rng,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(body.issued_at, 1_000);
        assert_eq!(body.ttl, 100);
        // (End-to-end TTL refusal is asserted in the gateway integration test.)
    }

    #[test]
    fn a_tampered_ticket_fails_to_open() {
        let mut rng = SeedRng(5);
        let key = TicketKey::generate(&mut rng);
        let mut ticket = issue_ticket(
            &key,
            &a_session(),
            &client_identity(),
            &posture(),
            &Purpose("investigation".into()),
            1_000,
            3_600,
            &mut rng,
        )
        .unwrap();
        let last = ticket.len() - 1;
        ticket[last] ^= 0x01;
        assert!(
            open_ticket(&key, &ticket).is_err(),
            "AEAD tag must reject tamper"
        );
    }
}
