//! The KEM-based mutual-authentication handshake, modelled as typed messages and
//! two step-driven state machines. Every message folds into a running transcript
//! hash; the identity signatures cover that transcript, so an on-path downgrade
//! or tamper breaks verification. The only path into an established [`Session`]
//! is one where every zero-trust check passed. [NFR-SEC-1] [AC-M-1] [AC-M-3]
//!
//! Protocol version 2 [MG-SEC-09] [MG-SEC-12]:
//! - the initiator names the service it means to reach and refuses any other, before
//!   it sends its key exchange [MG-SEC-01];
//! - certificates are checked against their validity window on every handshake
//!   [MG-SEC-02] and against the live revocation state [MG-SEC-03];
//! - the responder authorises a peer at the lower of the clearance its certificate
//!   grants and the posture it requests [MG-SEC-04];
//! - both sides require a hybrid (ML-KEM + X25519, X-Wing combined) suite unless a
//!   caller explicitly opts out [MG-SEC-12].

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use policy::{
    AccessRequest, Action, Clearance, PolicyDecisionPoint, Purpose, Resource, Subject, SubjectId,
};
use pqc_agility::hash::{ct_eq, hkdf_sha256_32, hmac_sha256, Digest, Sha256};
use pqc_agility::{
    negotiate_kem, Capabilities, Ciphertext, CryptoProvider, KemPublicKey, KemSecretKey,
    KemSuiteId, Rng, SecretArray, SigId, Signature, SigningKey,
};

use crate::error::{AuthzError, MtlsError};
use crate::identity::{
    canonical_cert_body, effective_clearance, verify_cert, IdentityCert, ServiceId,
    WorkloadIdentity,
};
use crate::posture::Posture;
use crate::session::{Authorization, PeerCert, Session};

/// The handshake protocol version this crate speaks. Version 2 carries certificate
/// terms, the X-Wing hybrid combiner and the hybrid requirement; a peer offering any
/// other version is refused. [MG-SEC-09] [MG-SEC-12]
pub const PROTOCOL_VERSION: u8 = 2;

/// Seconds since the Unix epoch by the system clock: the time certificates are
/// checked at unless a caller fixes it with `at_time`.
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The suites a side offers: all its post-quantum suites, or only the hybrid ones.
fn offered_suites(provider: &CryptoProvider, require_hybrid: bool) -> Vec<KemSuiteId> {
    provider
        .capabilities()
        .kem_suites
        .into_iter()
        .filter(|s| s.is_post_quantum() && (!require_hybrid || s.is_hybrid()))
        .collect()
}

// ---- Messages -------------------------------------------------------------

/// First flight: protocol version, client capabilities, declared purpose, and
/// requested posture.
#[derive(Debug, Clone)]
pub struct ClientHello {
    /// Handshake protocol version ([`PROTOCOL_VERSION`]).
    pub version: u8,
    /// Client nonce.
    pub nonce_c: [u8; 32],
    /// Offered capabilities.
    pub capabilities: Capabilities,
    /// Declared purpose of the connection.
    pub purpose: Purpose,
    /// Requested posture. The responder authorises at most what the client's
    /// certificate grants, whatever is asked for here. [MG-SEC-04]
    pub posture: Posture,
}

/// Server response: chosen suite, ephemeral KEM key, cert, transcript signature.
#[derive(Debug, Clone)]
pub struct ServerHello {
    /// Server nonce.
    pub nonce_s: [u8; 32],
    /// Negotiated suite.
    pub suite: KemSuiteId,
    /// Ephemeral KEM public key.
    pub kem_pk: KemPublicKey,
    /// Server certificate.
    pub cert: IdentityCert,
    /// Algorithm of `sig_r`.
    pub sig_alg: SigId,
    /// Server signature over the transcript so far.
    pub sig_r: Signature,
}

/// Client key exchange: encapsulation, cert, transcript signature, key confirm.
#[derive(Debug, Clone)]
pub struct ClientKeyExchange {
    /// KEM ciphertext.
    pub ct: Ciphertext,
    /// Client certificate (mandatory for mutual auth; `None` is rejected).
    pub cert: Option<IdentityCert>,
    /// Algorithm of `sig_i`.
    pub sig_alg: SigId,
    /// Client signature over the transcript so far.
    pub sig_i: Signature,
    /// Client key-confirmation MAC.
    pub finished_i: [u8; 32],
}

/// Final flight: server key-confirmation MAC.
#[derive(Debug, Clone)]
pub struct Finished {
    /// Server key-confirmation MAC.
    pub finished_r: [u8; 32],
}

/// A tagged handshake message, for transport over a [`crate::channel::Channel`].
#[derive(Debug, Clone)]
pub enum HandshakeMessage {
    /// `ClientHello`.
    ClientHello(ClientHello),
    /// `ServerHello`.
    ServerHello(ServerHello),
    /// `ClientKeyExchange`.
    ClientKeyExchange(ClientKeyExchange),
    /// `Finished`.
    Finished(Finished),
}

// ---- Transcript folding ---------------------------------------------------

fn fold_client_hello(t: &mut Sha256, m: &ClientHello) {
    t.update(b"midgard-pqmtls-transcript");
    t.update(&[m.version]);
    t.update(&m.nonce_c);
    for s in &m.capabilities.kem_suites {
        t.update(&[s.as_u8()]);
    }
    t.update(&[0xff]);
    for a in &m.capabilities.sig_algs {
        t.update(&[a.as_u8()]);
    }
    t.update(&[0xff]);
    let pb = m.purpose.0.as_bytes();
    t.update(&(pb.len() as u32).to_le_bytes());
    t.update(pb);
    t.update(&[m.posture.level.0]);
    for c in &m.posture.compartments {
        let cb = c.0.as_bytes();
        t.update(&(cb.len() as u32).to_le_bytes());
        t.update(cb);
    }
    t.update(&[0xff]);
}

fn fold_server_hello_pre(
    t: &mut Sha256,
    nonce_s: &[u8; 32],
    suite: KemSuiteId,
    kem_pk: &KemPublicKey,
    cert: &IdentityCert,
) {
    t.update(nonce_s);
    t.update(&[suite.as_u8()]);
    t.update(kem_pk.as_bytes());
    t.update(&canonical_cert_body(&cert.identity, &cert.terms));
}

fn fold_cke_pre(t: &mut Sha256, ct: &Ciphertext, cert: &IdentityCert) {
    t.update(ct.as_bytes());
    t.update(&canonical_cert_body(&cert.identity, &cert.terms));
}

fn sign_context(label: &[u8], td: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(label.len() + 32);
    v.extend_from_slice(label);
    v.extend_from_slice(td);
    v
}

fn derive_traffic(ss: &[u8], td: &[u8; 32]) -> SecretArray<32> {
    SecretArray::new(hkdf_sha256_32(td, ss, b"midgard-pqmtls-traffic-v2"))
}

fn mac(key: &[u8; 32], label: &[u8]) -> [u8; 32] {
    hmac_sha256(key, label)
}

pub(crate) fn authorize(
    pdp: &dyn PolicyDecisionPoint,
    peer: &WorkloadIdentity,
    purpose: &Purpose,
    posture: &Posture,
    resource: &Resource,
) -> Result<(), MtlsError> {
    let mut purposes = BTreeSet::new();
    purposes.insert(purpose.clone());
    let subject = Subject {
        id: SubjectId(peer.id.0.clone()),
        roles: peer.roles.clone(),
        clearance: Clearance {
            level: posture.level,
            compartments: posture.compartments.clone(),
        },
        purposes,
    };
    let req = AccessRequest {
        subject: &subject,
        resource,
        action: Action("OpenChannel".into()),
        purpose: purpose.clone(),
    };
    if pdp.decide(&req).is_permit() {
        Ok(())
    } else {
        Err(MtlsError::Authz(AuthzError::Denied))
    }
}

// ---- Initiator (client) ---------------------------------------------------

/// The connecting side of the handshake.
pub struct Initiator<'a> {
    provider: &'a CryptoProvider,
    cert: IdentityCert,
    signing_key: &'a SigningKey,
    trust_anchor: crate::identity::TrustAnchor,
    expected_peer: ServiceId,
    purpose: Purpose,
    posture: Posture,
    rng: &'a mut dyn Rng,
    require_hybrid: bool,
    now: Option<u64>,
    transcript: Sha256,
    traffic: Option<SecretArray<32>>,
    peer: Option<(WorkloadIdentity, PeerCert)>,
    suite: Option<KemSuiteId>,
}

impl<'a> Initiator<'a> {
    /// Build an initiator that will only complete a handshake with `expected_peer`:
    /// a responder holding a valid certificate for any other service is refused before
    /// the initiator sends its key exchange. [MG-SEC-01]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: &'a CryptoProvider,
        cert: IdentityCert,
        signing_key: &'a SigningKey,
        trust_anchor: crate::identity::TrustAnchor,
        expected_peer: ServiceId,
        purpose: Purpose,
        posture: Posture,
        rng: &'a mut dyn Rng,
    ) -> Self {
        Self {
            provider,
            cert,
            signing_key,
            trust_anchor,
            expected_peer,
            purpose,
            posture,
            rng,
            require_hybrid: true,
            now: None,
            transcript: Sha256::new(),
            traffic: None,
            peer: None,
            suite: None,
        }
    }

    /// Accept a pure ML-KEM suite as well as the hybrid ones. Hybrid is required by
    /// default; only interop tests and explicitly configured deployments turn it off.
    /// [MG-SEC-12]
    pub fn allow_non_hybrid(mut self) -> Self {
        self.require_hybrid = false;
        self
    }

    /// Check certificates at `now` (Unix seconds) instead of the system clock.
    pub fn at_time(mut self, now: u64) -> Self {
        self.now = Some(now);
        self
    }

    /// Produce the first flight.
    pub fn start(&mut self) -> ClientHello {
        let mut nonce_c = [0u8; 32];
        self.rng.fill(&mut nonce_c);
        let mut capabilities = self.provider.capabilities();
        capabilities.kem_suites = offered_suites(self.provider, self.require_hybrid);
        let m = ClientHello {
            version: PROTOCOL_VERSION,
            nonce_c,
            capabilities,
            purpose: self.purpose.clone(),
            posture: self.posture.clone(),
        };
        fold_client_hello(&mut self.transcript, &m);
        m
    }

    /// Process the server hello and produce the client key exchange.
    pub fn on_server_hello(&mut self, sh: ServerHello) -> Result<ClientKeyExchange, MtlsError> {
        let now = self.now.unwrap_or_else(unix_now);
        verify_cert(&sh.cert, &self.trust_anchor, self.provider, now)?;
        // A valid mesh certificate is not enough: it must be the service we meant to
        // call. Checked before anything else is sent. [MG-SEC-01]
        if sh.cert.identity.id != self.expected_peer {
            return Err(MtlsError::PeerIdentityMismatch {
                expected: self.expected_peer.0.clone(),
                got: sh.cert.identity.id.0.clone(),
            });
        }
        fold_server_hello_pre(
            &mut self.transcript,
            &sh.nonce_s,
            sh.suite,
            &sh.kem_pk,
            &sh.cert,
        );
        let td = self.transcript.clone().finalize();
        let scheme = self
            .provider
            .sig_for(sh.sig_alg)
            .map_err(|_| MtlsError::PeerSignatureInvalid)?;
        scheme
            .verify(
                &sh.cert.identity.verifying_key,
                &sign_context(b"server-auth", &td),
                &sh.sig_r,
            )
            .map_err(|_| MtlsError::PeerSignatureInvalid)?;

        if !offered_suites(self.provider, self.require_hybrid).contains(&sh.suite)
            || !sh.suite.is_post_quantum()
        {
            return Err(if self.require_hybrid && !sh.suite.is_hybrid() {
                MtlsError::HybridRequired
            } else {
                MtlsError::DowngradeDetected
            });
        }

        let kem = self
            .provider
            .kem_for(sh.suite)
            .map_err(|_| MtlsError::BackendUnavailable)?;
        let (ct, ss) = kem
            .encapsulate(&sh.kem_pk, &mut *self.rng)
            .map_err(MtlsError::Crypto)?;

        fold_cke_pre(&mut self.transcript, &ct, &self.cert);
        let td2 = self.transcript.clone().finalize();

        let my_scheme = self
            .provider
            .sig_for(self.cert.identity.sig_alg)
            .map_err(|_| MtlsError::BackendUnavailable)?;
        let sig_i = my_scheme
            .sign(self.signing_key, &sign_context(b"client-auth", &td2))
            .map_err(MtlsError::Crypto)?;

        let traffic = derive_traffic(ss.expose(), &td2);
        let finished_i = mac(traffic.expose(), b"client-finished");

        self.traffic = Some(traffic);
        self.peer = Some((
            sh.cert.identity.clone(),
            PeerCert {
                fingerprint: sh.cert.fingerprint(),
                not_after: sh.cert.terms.not_after,
            },
        ));
        self.suite = Some(sh.suite);

        Ok(ClientKeyExchange {
            ct,
            cert: Some(self.cert.clone()),
            sig_alg: self.cert.identity.sig_alg,
            sig_i,
            finished_i,
        })
    }

    /// Process the final flight and produce the session.
    pub fn on_finished(&mut self, fin: Finished) -> Result<Session, MtlsError> {
        let traffic = self
            .traffic
            .take()
            .ok_or_else(|| MtlsError::Protocol("no traffic secret".into()))?;
        let expected = mac(traffic.expose(), b"server-finished");
        if !ct_eq(&expected, &fin.finished_r) {
            return Err(MtlsError::KeyConfirmationFailed);
        }
        let (peer, peer_cert) = self
            .peer
            .take()
            .ok_or_else(|| MtlsError::Protocol("no peer".into()))?;
        let suite = self
            .suite
            .ok_or_else(|| MtlsError::Protocol("no suite".into()))?;
        Ok(Session::new(peer, peer_cert, None, suite, traffic))
    }
}

// ---- Responder (server) ---------------------------------------------------

/// The accepting side of the handshake.
pub struct Responder<'a> {
    provider: &'a CryptoProvider,
    cert: IdentityCert,
    signing_key: &'a SigningKey,
    trust_anchor: crate::identity::TrustAnchor,
    pdp: &'a dyn PolicyDecisionPoint,
    channel_resource: Resource,
    rng: &'a mut dyn Rng,
    require_hybrid: bool,
    now: Option<u64>,
    transcript: Sha256,
    kem_sk: Option<KemSecretKey>,
    suite: Option<KemSuiteId>,
    client_purpose: Option<Purpose>,
    client_posture: Option<Posture>,
}

impl<'a> Responder<'a> {
    /// Build a responder.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: &'a CryptoProvider,
        cert: IdentityCert,
        signing_key: &'a SigningKey,
        trust_anchor: crate::identity::TrustAnchor,
        pdp: &'a dyn PolicyDecisionPoint,
        channel_resource: Resource,
        rng: &'a mut dyn Rng,
    ) -> Self {
        Self {
            provider,
            cert,
            signing_key,
            trust_anchor,
            pdp,
            channel_resource,
            rng,
            require_hybrid: true,
            now: None,
            transcript: Sha256::new(),
            kem_sk: None,
            suite: None,
            client_purpose: None,
            client_posture: None,
        }
    }

    /// Accept a pure ML-KEM suite as well as the hybrid ones (see
    /// [`Initiator::allow_non_hybrid`]). [MG-SEC-12]
    pub fn allow_non_hybrid(mut self) -> Self {
        self.require_hybrid = false;
        self
    }

    /// Check certificates at `now` (Unix seconds) instead of the system clock.
    pub fn at_time(mut self, now: u64) -> Self {
        self.now = Some(now);
        self
    }

    /// Process the client hello and produce the server hello.
    pub fn on_client_hello(&mut self, ch: ClientHello) -> Result<ServerHello, MtlsError> {
        if ch.version != PROTOCOL_VERSION {
            return Err(MtlsError::UnsupportedVersion(ch.version));
        }
        fold_client_hello(&mut self.transcript, &ch);
        let local = offered_suites(self.provider, self.require_hybrid);
        let suite = negotiate_kem(&local, &ch.capabilities.kem_suites).map_err(|e| {
            let offered = &ch.capabilities.kem_suites;
            if self.require_hybrid && !offered.is_empty() && !offered.iter().any(|s| s.is_hybrid())
            {
                MtlsError::HybridRequired
            } else {
                MtlsError::Negotiation(e)
            }
        })?;
        let kem = self
            .provider
            .kem_for(suite)
            .map_err(|_| MtlsError::BackendUnavailable)?;
        let (kem_pk, kem_sk) = kem.keypair(&mut *self.rng).map_err(MtlsError::Crypto)?;
        let mut nonce_s = [0u8; 32];
        self.rng.fill(&mut nonce_s);

        fold_server_hello_pre(&mut self.transcript, &nonce_s, suite, &kem_pk, &self.cert);
        let td = self.transcript.clone().finalize();
        let my_scheme = self
            .provider
            .sig_for(self.cert.identity.sig_alg)
            .map_err(|_| MtlsError::BackendUnavailable)?;
        let sig_r = my_scheme
            .sign(self.signing_key, &sign_context(b"server-auth", &td))
            .map_err(MtlsError::Crypto)?;

        self.kem_sk = Some(kem_sk);
        self.suite = Some(suite);
        self.client_purpose = Some(ch.purpose);
        self.client_posture = Some(ch.posture);

        Ok(ServerHello {
            nonce_s,
            suite,
            kem_pk,
            cert: self.cert.clone(),
            sig_alg: self.cert.identity.sig_alg,
            sig_r,
        })
    }

    /// Process the client key exchange, authorize, and produce the final flight
    /// plus the established session.
    pub fn on_client_key_exchange(
        &mut self,
        cke: ClientKeyExchange,
    ) -> Result<(Finished, Session), MtlsError> {
        let cert = cke.cert.clone().ok_or(MtlsError::MissingPeerCredential)?;
        let now = self.now.unwrap_or_else(unix_now);
        verify_cert(&cert, &self.trust_anchor, self.provider, now)?;

        fold_cke_pre(&mut self.transcript, &cke.ct, &cert);
        let td2 = self.transcript.clone().finalize();
        let scheme = self
            .provider
            .sig_for(cke.sig_alg)
            .map_err(|_| MtlsError::PeerSignatureInvalid)?;
        scheme
            .verify(
                &cert.identity.verifying_key,
                &sign_context(b"client-auth", &td2),
                &cke.sig_i,
            )
            .map_err(|_| MtlsError::PeerSignatureInvalid)?;

        let suite = self
            .suite
            .ok_or_else(|| MtlsError::Protocol("no suite".into()))?;
        let kem_sk = self
            .kem_sk
            .take()
            .ok_or_else(|| MtlsError::Protocol("no kem secret".into()))?;
        let kem = self
            .provider
            .kem_for(suite)
            .map_err(|_| MtlsError::BackendUnavailable)?;
        let ss = kem
            .decapsulate(&kem_sk, &cke.ct)
            .map_err(MtlsError::Crypto)?;
        let traffic = derive_traffic(ss.expose(), &td2);

        let expect_fin_i = mac(traffic.expose(), b"client-finished");
        if !ct_eq(&expect_fin_i, &cke.finished_i) {
            return Err(MtlsError::KeyConfirmationFailed);
        }

        let purpose = self
            .client_purpose
            .clone()
            .ok_or_else(|| MtlsError::Protocol("no purpose".into()))?;
        let requested = self
            .client_posture
            .clone()
            .ok_or_else(|| MtlsError::Protocol("no posture".into()))?;
        // The peer is authorised at what its certificate grants, capped by what it
        // asked for — never at a level it merely claims. [MG-SEC-04]
        let clearance = effective_clearance(&cert.terms.clearance, &requested);
        authorize(
            self.pdp,
            &cert.identity,
            &purpose,
            &clearance,
            &self.channel_resource,
        )?;

        let finished_r = mac(traffic.expose(), b"server-finished");
        let peer_cert = PeerCert {
            fingerprint: cert.fingerprint(),
            not_after: cert.terms.not_after,
        };
        let session = Session::new(
            cert.identity,
            peer_cert,
            Some(Authorization { clearance, purpose }),
            suite,
            traffic,
        );
        Ok((Finished { finished_r }, session))
    }
}

/// Drive a full handshake between two parties in-process (the loopback driver).
/// Returns `(client_session, server_session)`.
pub fn mutual_handshake(
    init: &mut Initiator,
    resp: &mut Responder,
) -> Result<(Session, Session), MtlsError> {
    let ch = init.start();
    let sh = resp.on_client_hello(ch)?;
    let cke = init.on_server_hello(sh)?;
    let (fin, resp_session) = resp.on_client_key_exchange(cke)?;
    let init_session = init.on_finished(fin)?;
    Ok((init_session, resp_session))
}
