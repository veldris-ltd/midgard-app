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
//!
//! An assertion is scoped and single-use: it names its issuer and audience, carries a
//! validity window no longer than [`MAX_ASSERTION_LIFETIME`] and a nonce, and the
//! bridge refuses one that is mis-scoped, stale, not yet valid or already seen.
//! [VLD-ANB-MIDGARD-0049] [AC-17.1] [AC-17.2] [AC-17.3] [AC-17.4]

use std::collections::{BTreeMap, BTreeSet};

use policy::{Clearance, Purpose};
use pqc_agility::{CryptoProvider, SigId, Signature, VerifyingKey};

use crate::idp::{Idp, IssueError};
use crate::store::UserRecord;

/// The assertion payload version. A payload without it (the earlier, unscoped shape)
/// is [`FederationError::Malformed`]; no producer of that shape exists.
pub const ASSERTION_VERSION: u8 = 0x02;
/// How far ahead of the bridge's clock an upstream's `issued_at` may be, in seconds.
pub const MAX_CLOCK_SKEW: u64 = 60;
/// The longest validity window (`expires_at - issued_at`) accepted, in seconds.
pub const MAX_ASSERTION_LIFETIME: u64 = 300;
/// The most unexpired nonces the replay cache holds. When it is full after purging
/// expired entries, verification fails closed rather than evicting a live nonce,
/// because eviction would re-admit a replay under load.
pub const MAX_SEEN: usize = 4096;

/// Claims resolved from a verified upstream assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedClaims {
    /// The upstream that issued the assertion; must equal the bridge's configured
    /// issuer.
    pub issuer: String,
    /// The relying party the assertion is for; must equal the bridge's configured
    /// audience.
    pub audience: String,
    /// When the upstream issued the assertion (Unix seconds).
    pub issued_at: u64,
    /// When the assertion stops being valid (Unix seconds, exclusive).
    pub expires_at: u64,
    /// A single-use value; a second assertion with the same nonce inside its validity
    /// is a replay.
    pub nonce: [u8; 16],
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
    /// The assertion was malformed, including a payload without the current version.
    Malformed,
    /// The assertion was issued by an upstream other than the configured one.
    WrongIssuer,
    /// The assertion is for a relying party other than this bridge.
    WrongAudience,
    /// The validity window is empty or longer than [`MAX_ASSERTION_LIFETIME`].
    LifetimeTooLong,
    /// `issued_at` is further ahead of the bridge's clock than [`MAX_CLOCK_SKEW`].
    NotYetValid,
    /// The assertion's validity has ended.
    Expired,
    /// The nonce was already accepted inside its validity, or the replay cache is
    /// full of unexpired nonces (fail closed).
    Replayed,
    /// Local token issuance was refused after a valid assertion.
    Issue(IssueError),
}

/// A trusted upstream identity provider that can vouch for a principal.
pub trait FederatedIdp {
    /// Verify a raw upstream assertion at time `now` (Unix seconds) and return the
    /// mapped local claims, or fail closed. Takes `&mut self` because an
    /// implementation keeps the state that makes an assertion single-use.
    fn verify(&mut self, assertion: &[u8], now: u64) -> Result<FederatedClaims, FederationError>;
}

/// A reference federation bridge trusting an upstream that signs assertions with an
/// ML-DSA key. The assertion is `version ‖ issuer ‖ audience ‖ issued_at ‖ expires_at
/// ‖ nonce ‖ subject ‖ attributes` signed by the upstream; the bridge checks its scope,
/// freshness and nonce, and maps a fixed attribute set to local
/// roles/clearance/purposes.
///
/// The replay cache lives in the instance, so it is only as wide as the instance is
/// shared: a deployment must hold one `PqFederation` per upstream behind a `Mutex`
/// and route every federated login through it. Separate instances, or separate
/// processes, each admit a given assertion once.
pub struct PqFederation {
    provider: CryptoProvider,
    upstream_key: VerifyingKey,
    upstream_alg: SigId,
    expected_issuer: String,
    expected_audience: String,
    /// Accepted nonces still inside their validity: nonce -> `expires_at`.
    seen: BTreeMap<[u8; 16], u64>,
}

impl PqFederation {
    /// Trust `upstream_key` for assertions signed with `upstream_alg`, issued by
    /// `expected_issuer` for `expected_audience`.
    pub fn new(
        provider: CryptoProvider,
        upstream_key: VerifyingKey,
        upstream_alg: SigId,
        expected_issuer: &str,
        expected_audience: &str,
    ) -> Self {
        Self {
            provider,
            upstream_key,
            upstream_alg,
            expected_issuer: expected_issuer.to_owned(),
            expected_audience: expected_audience.to_owned(),
            seen: BTreeMap::new(),
        }
    }

    /// How many nonces the replay cache holds (accepted and not yet purged).
    pub fn seen_len(&self) -> usize {
        self.seen.len()
    }

    /// The bytes an upstream signs over: `version ‖ issuer ‖ audience ‖ issued_at ‖
    /// expires_at ‖ nonce ‖ subject ‖ roles ‖ level ‖ compartments ‖ purposes`,
    /// canonically framed (integers little-endian). Exposed so a test/upstream can
    /// produce a valid assertion.
    pub fn assertion_payload(claims: &FederatedClaims) -> Vec<u8> {
        let mut o = vec![ASSERTION_VERSION];
        put_bytes(&mut o, claims.issuer.as_bytes());
        put_bytes(&mut o, claims.audience.as_bytes());
        o.extend_from_slice(&claims.issued_at.to_le_bytes());
        o.extend_from_slice(&claims.expires_at.to_le_bytes());
        o.extend_from_slice(&claims.nonce);
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
    fn verify(&mut self, assertion: &[u8], now: u64) -> Result<FederatedClaims, FederationError> {
        // Forget nonces whose assertions can no longer be accepted anyway, on every
        // call, so the cache only ever holds live entries.
        purge_seen(&mut self.seen, now);

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

        // Only a payload whose signature verifies is parsed and judged, in this order.
        let claims = decode_claims(payload).ok_or(FederationError::Malformed)?;
        if claims.issuer != self.expected_issuer {
            return Err(FederationError::WrongIssuer);
        }
        if claims.audience != self.expected_audience {
            return Err(FederationError::WrongAudience);
        }
        if claims.expires_at <= claims.issued_at
            || claims.expires_at - claims.issued_at > MAX_ASSERTION_LIFETIME
        {
            return Err(FederationError::LifetimeTooLong);
        }
        if now.saturating_add(MAX_CLOCK_SKEW) < claims.issued_at {
            return Err(FederationError::NotYetValid);
        }
        if now >= claims.expires_at {
            return Err(FederationError::Expired);
        }
        admit_nonce(&mut self.seen, claims.nonce, claims.expires_at)?;
        Ok(claims)
    }
}

/// Drop every nonce whose assertion expired at or before `now`.
fn purge_seen(seen: &mut BTreeMap<[u8; 16], u64>, now: u64) {
    seen.retain(|_, expires_at| *expires_at > now);
}

/// Record `nonce` as used until `expires_at`, refusing one already recorded. A cache
/// already holding [`MAX_SEEN`] live nonces refuses too, rather than evicting one.
fn admit_nonce(
    seen: &mut BTreeMap<[u8; 16], u64>,
    nonce: [u8; 16],
    expires_at: u64,
) -> Result<(), FederationError> {
    if seen.contains_key(&nonce) || seen.len() >= MAX_SEEN {
        return Err(FederationError::Replayed);
    }
    seen.insert(nonce, expires_at);
    Ok(())
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
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
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
    if r.u8()? != ASSERTION_VERSION {
        return None;
    }
    let issuer = r.string()?;
    let audience = r.string()?;
    let issued_at = r.u64()?;
    let expires_at = r.u64()?;
    let nonce: [u8; 16] = r.take(16)?.try_into().ok()?;
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
        issuer,
        audience,
        issued_at,
        expires_at,
        nonce,
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
/// `now` is both the assertion's verification time and the token's issue time.
/// [RC-29]
pub fn federated_login(
    idp: &Idp,
    federation: &mut dyn FederatedIdp,
    assertion: &[u8],
    purpose: Purpose,
    now: u64,
    ttl_secs: u64,
) -> Result<Vec<u8>, FederationError> {
    let claims = federation.verify(assertion, now)?;
    idp.mint_token(&claims.subject, &claims.user, purpose, now, ttl_secs)
        .map_err(FederationError::Issue)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cache refuses a seen nonce, forgets it at expiry and, when full of live
    // nonces, refuses a new one rather than evicting. [AC-17.2] [VLD-ANB-MIDGARD-0049]
    #[test]
    fn a_full_replay_cache_fails_closed_until_entries_expire() {
        let mut seen = BTreeMap::new();
        for i in 0..MAX_SEEN as u32 {
            let mut nonce = [0u8; 16];
            nonce[..4].copy_from_slice(&i.to_le_bytes());
            admit_nonce(&mut seen, nonce, 1_000).unwrap();
        }
        assert_eq!(
            admit_nonce(&mut seen, [0u8; 16], 1_000),
            Err(FederationError::Replayed)
        );
        let fresh = [0xAA; 16];
        assert_eq!(
            admit_nonce(&mut seen, fresh, 1_200),
            Err(FederationError::Replayed)
        );
        assert_eq!(seen.len(), MAX_SEEN);

        purge_seen(&mut seen, 999);
        assert_eq!(seen.len(), MAX_SEEN);
        purge_seen(&mut seen, 1_000);
        assert!(seen.is_empty());
        admit_nonce(&mut seen, fresh, 1_200).unwrap();
        assert_eq!(seen.len(), 1);
    }
}
