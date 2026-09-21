//! PQ-signed access tokens. A token is a canonical claims blob plus an ML-DSA
//! signature over a domain-separated preimage — the post-quantum analogue of a
//! signed JWT, but self-contained (no JOSE, no external libs). Verification is
//! fully offline: it needs only the issuer's verifying key. [NFR-SEC-2] [NFR-PRV-1]

use std::collections::BTreeSet;

use policy::{ClassificationLevel, Clearance, Compartment, Purpose, Role, Subject, SubjectId};
use pqc_agility::{CryptoProvider, SigId, Signature, SigningKey, VerifyingKey};

/// Domain separator so a token signature can never be confused with any other
/// ML-DSA signature the platform produces (certs, artefacts, audit entries).
const TOKEN_DOMAIN: &[u8] = b"midgard-token-v1";

/// The asserted claims. Times are Unix seconds; the clock is injected so signing
/// and verification are deterministic and testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenClaims {
    /// Issuing IdP id.
    pub issuer: String,
    /// The authenticated principal (user or workload).
    pub subject: String,
    /// Roles granted to the subject.
    pub roles: BTreeSet<Role>,
    /// Clearance level.
    pub level: ClassificationLevel,
    /// Held compartments.
    pub compartments: BTreeSet<Compartment>,
    /// The purpose this token authorises.
    pub purpose: Purpose,
    /// Issued-at (Unix seconds).
    pub issued_at: u64,
    /// Expiry (Unix seconds, exclusive).
    pub expires_at: u64,
}

impl TokenClaims {
    /// Project the claims into a policy [`Subject`] for authorization decisions.
    pub fn to_subject(&self) -> Subject {
        let mut purposes = BTreeSet::new();
        purposes.insert(self.purpose.clone());
        Subject {
            id: SubjectId(self.subject.clone()),
            roles: self.roles.clone(),
            clearance: Clearance {
                level: self.level,
                compartments: self.compartments.clone(),
            },
            purposes,
        }
    }
}

/// Token errors — every variant is a fail-closed denial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// The token bytes could not be parsed.
    Malformed,
    /// The signature algorithm is unknown/unavailable.
    UnknownAlg,
    /// The signature did not verify against the issuer key.
    BadSignature,
    /// `now` is before `issued_at`.
    NotYetValid,
    /// `now` is at or after `expires_at`.
    Expired,
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenError::Malformed => write!(f, "malformed token"),
            TokenError::UnknownAlg => write!(f, "unknown signature algorithm"),
            TokenError::BadSignature => write!(f, "token signature invalid"),
            TokenError::NotYetValid => write!(f, "token not yet valid"),
            TokenError::Expired => write!(f, "token expired"),
        }
    }
}

impl std::error::Error for TokenError {}

// ---- canonical codec (local, minimal) -------------------------------------

fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(o: &mut Vec<u8>, v: u64) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_str(o: &mut Vec<u8>, s: &str) {
    put_u32(o, s.len() as u32);
    o.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], TokenError> {
        let end = self.pos.checked_add(n).ok_or(TokenError::Malformed)?;
        if end > self.b.len() {
            return Err(TokenError::Malformed);
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, TokenError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, TokenError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Result<u64, TokenError> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_le_bytes(a))
    }
    fn bytes(&mut self) -> Result<Vec<u8>, TokenError> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn string(&mut self) -> Result<String, TokenError> {
        String::from_utf8(self.bytes()?).map_err(|_| TokenError::Malformed)
    }
}

/// Canonical claims encoding (BTreeSets iterate in sorted order → deterministic).
fn encode_claims(c: &TokenClaims) -> Vec<u8> {
    let mut o = Vec::new();
    put_str(&mut o, &c.issuer);
    put_str(&mut o, &c.subject);
    put_u32(&mut o, c.roles.len() as u32);
    for r in &c.roles {
        put_str(&mut o, &r.0);
    }
    o.push(c.level.0);
    put_u32(&mut o, c.compartments.len() as u32);
    for cm in &c.compartments {
        put_str(&mut o, &cm.0);
    }
    put_str(&mut o, &c.purpose.0);
    put_u64(&mut o, c.issued_at);
    put_u64(&mut o, c.expires_at);
    o
}

fn decode_claims(b: &[u8]) -> Result<TokenClaims, TokenError> {
    let mut r = Reader { b, pos: 0 };
    let issuer = r.string()?;
    let subject = r.string()?;
    let nr = r.u32()? as usize;
    let mut roles = BTreeSet::new();
    for _ in 0..nr {
        roles.insert(Role(r.string()?));
    }
    let level = ClassificationLevel(r.u8()?);
    let nc = r.u32()? as usize;
    let mut compartments = BTreeSet::new();
    for _ in 0..nc {
        compartments.insert(Compartment(r.string()?));
    }
    let purpose = Purpose(r.string()?);
    let issued_at = r.u64()?;
    let expires_at = r.u64()?;
    if r.pos != r.b.len() {
        return Err(TokenError::Malformed);
    }
    Ok(TokenClaims {
        issuer,
        subject,
        roles,
        level,
        compartments,
        purpose,
        issued_at,
        expires_at,
    })
}

fn preimage(claims_blob: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(TOKEN_DOMAIN.len() + claims_blob.len());
    v.extend_from_slice(TOKEN_DOMAIN);
    v.extend_from_slice(claims_blob);
    v
}

/// Sign `claims` into token bytes: `claims_blob ‖ sig_alg ‖ signature`.
pub fn sign_token(
    provider: &CryptoProvider,
    alg: SigId,
    signing_key: &SigningKey,
    claims: &TokenClaims,
) -> Result<Vec<u8>, TokenError> {
    let blob = encode_claims(claims);
    let scheme = provider.sig_for(alg).map_err(|_| TokenError::UnknownAlg)?;
    let sig = scheme
        .sign(signing_key, &preimage(&blob))
        .map_err(|_| TokenError::BadSignature)?;
    let mut o = Vec::new();
    put_u32(&mut o, blob.len() as u32);
    o.extend_from_slice(&blob);
    o.push(alg.as_u8());
    put_u32(&mut o, sig.as_bytes().len() as u32);
    o.extend_from_slice(sig.as_bytes());
    Ok(o)
}

/// Verify token bytes against the issuer's verifying key at time `now`. Checks
/// signature first, then the validity window. Fails closed on any defect.
pub fn verify_token(
    provider: &CryptoProvider,
    issuer_vk: &VerifyingKey,
    bytes: &[u8],
    now: u64,
) -> Result<TokenClaims, TokenError> {
    let mut r = Reader { b: bytes, pos: 0 };
    let blob = r.bytes()?;
    let alg = SigId::from_u8(r.u8()?).ok_or(TokenError::UnknownAlg)?;
    let sig_bytes = r.bytes()?;
    if r.pos != r.b.len() {
        return Err(TokenError::Malformed);
    }
    if alg != issuer_vk.alg() {
        return Err(TokenError::BadSignature);
    }
    let scheme = provider.sig_for(alg).map_err(|_| TokenError::UnknownAlg)?;
    let sig = Signature::new(alg, sig_bytes);
    scheme
        .verify(issuer_vk, &preimage(&blob), &sig)
        .map_err(|_| TokenError::BadSignature)?;
    let claims = decode_claims(&blob)?;
    if now < claims.issued_at {
        return Err(TokenError::NotYetValid);
    }
    if now >= claims.expires_at {
        return Err(TokenError::Expired);
    }
    Ok(claims)
}
