//! Enterprise-IdP **federation**. When configured, a principal authenticated by a [NFR-SEC-2]
//! trusted upstream IdP can obtain a local Midgard token without a local password:
//! the upstream presents a signed assertion, the bridge verifies it against the
//! upstream's key, maps its claims to local authorization attributes, and the local
//! IdP mints the usual ML-DSA token. When *not* configured there is no such path —
//! federation is strictly opt-in. [RC-29]
//!
//! The reference [`PqFederation`] trusts an upstream that signs assertions with an
//! ML-DSA key (the same PQ scheme the mesh uses); a SAML/OIDC upstream implements the
//! [`FederatedIdp`] trait by verifying its own assertion format and returning the
//! same [`FederatedClaims`].

use std::collections::BTreeSet;

use policy::{Clearance, Purpose};
use pqc_agility::{CryptoProvider, SigId, Signature, VerifyingKey};

use crate::idp::{Idp, IssueError};
use crate::store::UserRecord;

/// Claims resolved from a verified upstream assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedClaims {
    /// The federated subject id (namespaced by the upstream in practice).
    pub subject: String,
    /// Local authorization attributes mapped from the upstream's groups/claims.
    pub user: UserRecord,
}

/// Why federation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FederationError {
    /// The upstream assertion failed signature verification.
    BadAssertion,
    /// The assertion was malformed.
    Malformed,
    /// Local token issuance was refused after a valid assertion.
    Issue(IssueError),
}

/// A trusted upstream identity provider that can vouch for a principal.
pub trait FederatedIdp {
    /// Verify a raw upstream assertion and return the mapped local claims, or fail
    /// closed.
    fn verify(&self, assertion: &[u8]) -> Result<FederatedClaims, FederationError>;
}

/// A reference federation bridge trusting an upstream that signs assertions with an
/// ML-DSA key. The assertion is `subject ‖ attributes` signed by the upstream; the
/// bridge maps a fixed attribute set to local roles/clearance/purposes.
pub struct PqFederation {
    provider: CryptoProvider,
    upstream_key: VerifyingKey,
    upstream_alg: SigId,
}

impl PqFederation {
    /// Trust `upstream_key` for assertions signed with `upstream_alg`.
    pub fn new(provider: CryptoProvider, upstream_key: VerifyingKey, upstream_alg: SigId) -> Self {
        Self {
            provider,
            upstream_key,
            upstream_alg,
        }
    }

    /// The bytes an upstream signs over: `subject ‖ roles ‖ level ‖ compartments ‖
    /// purposes`, canonically framed. Exposed so a test/upstream can produce a valid
    /// assertion.
    pub fn assertion_payload(claims: &FederatedClaims) -> Vec<u8> {
        let mut o = Vec::new();
        put_bytes(&mut o, claims.subject.as_bytes());
        put_u32(&mut o, claims.user.roles.len() as u32);
        for r in &claims.user.roles {
            put_bytes(&mut o, r.0.as_bytes());
        }
        o.push(claims.user.clearance.level.0);
        put_u32(&mut o, claims.user.clearance.compartments.len() as u32);
        for c in &claims.user.clearance.compartments {
            put_bytes(&mut o, c.0.as_bytes());
        }
        put_u32(&mut o, claims.user.purposes.len() as u32);
        for p in &claims.user.purposes {
            put_bytes(&mut o, p.0.as_bytes());
        }
        o
    }

    /// Encode a full signed assertion (`payload ‖ signature`) — the wire form an
    /// upstream transmits.
    pub fn encode_assertion(claims: &FederatedClaims, signature: &Signature) -> Vec<u8> {
        let payload = Self::assertion_payload(claims);
        let mut o = Vec::new();
        o.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        o.extend_from_slice(&payload);
        o.push(signature.alg().as_u8());
        o.extend_from_slice(&(signature.as_bytes().len() as u32).to_le_bytes());
        o.extend_from_slice(signature.as_bytes());
        o
    }
}

impl FederatedIdp for PqFederation {
    fn verify(&self, assertion: &[u8]) -> Result<FederatedClaims, FederationError> {
        // Split payload ‖ signature.
        if assertion.len() < 4 {
            return Err(FederationError::Malformed);
        }
        let plen =
            u32::from_le_bytes([assertion[0], assertion[1], assertion[2], assertion[3]]) as usize;
        let mut pos = 4;
        let payload = assertion
            .get(pos..pos + plen)
            .ok_or(FederationError::Malformed)?;
        pos += plen;
        let sig_alg = SigId::from_u8(*assertion.get(pos).ok_or(FederationError::Malformed)?)
            .ok_or(FederationError::Malformed)?;
        pos += 1;
        let slen = u32::from_le_bytes([
            *assertion.get(pos).ok_or(FederationError::Malformed)?,
            *assertion.get(pos + 1).ok_or(FederationError::Malformed)?,
            *assertion.get(pos + 2).ok_or(FederationError::Malformed)?,
            *assertion.get(pos + 3).ok_or(FederationError::Malformed)?,
        ]) as usize;
        pos += 4;
        let sig_bytes = assertion
            .get(pos..pos + slen)
            .ok_or(FederationError::Malformed)?;

        if sig_alg != self.upstream_alg {
            return Err(FederationError::BadAssertion);
        }
        let scheme = self
            .provider
            .sig_for(sig_alg)
            .map_err(|_| FederationError::BadAssertion)?;
        let signature = Signature::new(sig_alg, sig_bytes.to_vec());
        scheme
            .verify(&self.upstream_key, payload, &signature)
            .map_err(|_| FederationError::BadAssertion)?;

        decode_claims(payload).ok_or(FederationError::Malformed)
    }
}

fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    put_u32(o, b.len() as u32);
    o.extend_from_slice(b);
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.b.len())?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<usize> {
        let s = self.take(4)?;
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
    }
    fn string(&mut self) -> Option<String> {
        let n = self.u32()?;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

fn decode_claims(b: &[u8]) -> Option<FederatedClaims> {
    use policy::{ClassificationLevel, Compartment, Role};
    let mut r = Reader { b, pos: 0 };
    let subject = r.string()?;
    let nroles = r.u32()?;
    let mut roles = BTreeSet::new();
    for _ in 0..nroles {
        roles.insert(Role(r.string()?));
    }
    let level = ClassificationLevel(r.u8()?);
    let ncomp = r.u32()?;
    let mut compartments = BTreeSet::new();
    for _ in 0..ncomp {
        compartments.insert(Compartment(r.string()?));
    }
    let npur = r.u32()?;
    let mut purposes = BTreeSet::new();
    for _ in 0..npur {
        purposes.insert(Purpose(r.string()?));
    }
    if r.pos != b.len() {
        return None;
    }
    Some(FederatedClaims {
        subject,
        user: UserRecord {
            roles,
            clearance: Clearance {
                level,
                compartments,
            },
            purposes,
        },
    })
}

/// Verify an upstream `assertion` through `federation` and, on success, mint a local
/// ML-DSA token for the mapped principal. The whole federated login in one call.
/// [RC-29]
pub fn federated_login(
    idp: &Idp,
    federation: &dyn FederatedIdp,
    assertion: &[u8],
    purpose: Purpose,
    now: u64,
    ttl_secs: u64,
) -> Result<Vec<u8>, FederationError> {
    let claims = federation.verify(assertion)?;
    idp.mint_token(&claims.subject, &claims.user, purpose, now, ttl_secs)
        .map_err(FederationError::Issue)
}
