//! Multi-factor authentication: **TOTP** (RFC 6238) and **WebAuthn/passkey** [NFR-SEC-2]
//! assertion verification. Both are second factors checked *before* a token is
//! minted. [RC-29]
//!
//! ## TOTP
//! HOTP (RFC 4226) over HMAC-SHA256 with a 30-second time step and ±1-step skew.
//! The shared secret is provisioned once and base32-encoded into an `otpauth://`
//! URI for an authenticator app. A login accepts a code once: the step it matched is
//! persisted per principal and any step at or below it is refused.
//! [VLD-ANB-MIDGARD-0026]
//!
//! ## WebAuthn
//! A passkey here is an **ML-DSA** credential (the enclosure's PQ signature scheme
//! in place of COSE/ES256). `verify_assertion` performs the assertion checks of
//! WebAuthn Level 2 section 7.2: the client data is a `webauthn.get` whose challenge
//! binds to the server challenge and whose origin is one the [`WebAuthnPolicy`]
//! allows, the authenticator-data RP-ID hash matches, the user-present flag is set,
//! the user-verified flag is set when the policy requires it, the signature over
//! `authenticatorData ‖ SHA-256(clientDataJSON)` verifies, and the signature counter
//! has moved forward. The client data arrives already parsed ([`ClientData`]): the
//! HTTP edge reads it with the shared, depth-limited JSON parser, so this crate holds
//! no JSON parser of its own. Parsing of the COSE public key, CBOR attestation object,
//! and registration ceremony is out of scope (documented) — the assertion (login)
//! path is real. [RC-29] [RC-30] [VLD-ANB-MIDGARD-0084]
//!
//! The user-verified flag is the authenticator's statement that it checked its user;
//! for a software passkey whoever holds the key can set it. A policy may demand it,
//! and an assertion without it is then refused, but it is never counted as a factor
//! of its own, as for administrators (WP-10).

use pqc_agility::hash::{ct_eq, hmac_sha256, sha256};
use pqc_agility::{CryptoProvider, SigId, Signature, VerifyingKey};

use crate::enc::{b32, b64url};

// ---- TOTP -----------------------------------------------------------------

const TIME_STEP: u64 = 30;
const DIGITS: u32 = 6;

/// A TOTP enrollment: the shared secret plus its parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotpSecret {
    secret: Vec<u8>,
}

impl TotpSecret {
    /// Wrap raw secret bytes (≥16 bytes recommended).
    pub fn new(secret: Vec<u8>) -> Self {
        Self { secret }
    }

    /// The raw secret bytes (for durable persistence of the enrollment).
    pub fn secret(&self) -> &[u8] {
        &self.secret
    }

    /// The `otpauth://totp/...` provisioning URI for an authenticator app.
    pub fn provisioning_uri(&self, issuer: &str, account: &str) -> String {
        format!(
            "otpauth://totp/{issuer}:{account}?secret={}&issuer={issuer}&algorithm=SHA256&digits={DIGITS}&period={TIME_STEP}",
            b32(&self.secret)
        )
    }

    /// The current 6-digit code at `unix_time` (for tests / a local prompt).
    pub fn code_at(&self, unix_time: u64) -> String {
        hotp(&self.secret, unix_time / TIME_STEP)
    }

    /// Verify `code` against `unix_time`, allowing ±1 time step of clock skew.
    /// Constant-time comparison. Stateless: a verifier that must refuse a second use
    /// of a code calls [`Self::verify_step`]. [RC-29]
    pub fn verify(&self, code: &str, unix_time: u64) -> bool {
        self.verify_step(code, unix_time, None).is_some()
    }

    /// Verify `code` against `unix_time` with ±1 step of skew and return the time
    /// step it matched, refusing any step at or below `last_accepted`, the step this
    /// principal last logged in with. Persisting the returned step and passing it back
    /// next time makes every code single use (RFC 6238 section 5.2). Every candidate is
    /// compared, in constant time, before anything is returned, so the timing does not
    /// reveal which step matched; if the code matches more than one usable step the
    /// earliest is taken. [VLD-ANB-MIDGARD-0026] [RC-29]
    pub fn verify_step(
        &self,
        code: &str,
        unix_time: u64,
        last_accepted: Option<u64>,
    ) -> Option<u64> {
        let counter = unix_time / TIME_STEP;
        let mut found = 0u64;
        let mut matched = 0u64;
        // Latest first, so an earlier match overwrites a later one.
        for step in [counter + 1, counter, counter.saturating_sub(1)] {
            let expected = hotp(&self.secret, step);
            let usable = last_accepted.map_or(true, |last| step > last);
            let hit = u64::from(ct_eq(expected.as_bytes(), code.as_bytes()) & usable);
            let mask = hit.wrapping_neg();
            matched = (matched & !mask) | (step & mask);
            found |= hit;
        }
        (found == 1).then_some(matched)
    }
}

/// HOTP (RFC 4226) over HMAC-SHA256 with dynamic truncation → `DIGITS` digits.
fn hotp(secret: &[u8], counter: u64) -> String {
    let hs = hmac_sha256(secret, &counter.to_be_bytes());
    let offset = (hs[hs.len() - 1] & 0x0f) as usize;
    let bin = ((hs[offset] & 0x7f) as u32) << 24
        | (hs[offset + 1] as u32) << 16
        | (hs[offset + 2] as u32) << 8
        | (hs[offset + 3] as u32);
    let modulo = 10u32.pow(DIGITS);
    format!("{:0width$}", bin % modulo, width = DIGITS as usize)
}

// ---- WebAuthn (passkey) ---------------------------------------------------

/// A registered passkey: the credential id and its ML-DSA public key, bound to a
/// relying-party id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAuthnCredential {
    /// Opaque credential id (as returned at registration).
    pub credential_id: Vec<u8>,
    /// The passkey's public key (ML-DSA).
    pub public_key: VerifyingKey,
    /// The relying-party id this passkey is scoped to.
    pub rp_id: String,
    /// The signature counter of the last assertion accepted (0 until one is, and
    /// always 0 for an authenticator that keeps no counter). [VLD-ANB-MIDGARD-0084]
    pub sign_count: u32,
}

/// The client data of an assertion (`CollectedClientData`), parsed by the caller from
/// the raw `clientDataJSON`. [VLD-ANB-MIDGARD-0084]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientData {
    /// The ceremony type; an assertion is `webauthn.get`.
    pub type_: String,
    /// The challenge, base64url-decoded.
    pub challenge: Vec<u8>,
    /// The origin the browser ran the ceremony in.
    pub origin: String,
}

/// What a relying party accepts from an assertion. The default allows no origin and
/// requires user verification, so nothing verifies until a caller configures the
/// origins it serves (fail closed). [VLD-ANB-MIDGARD-0084]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAuthnPolicy {
    /// The exact origins (`scheme://host[:port]`) a ceremony may run in.
    pub origins: Vec<String>,
    /// Whether the authenticator must have verified its user (the UV flag).
    pub require_uv: bool,
}

impl Default for WebAuthnPolicy {
    fn default() -> Self {
        Self {
            origins: Vec::new(),
            require_uv: true,
        }
    }
}

/// An authentication assertion presented at login (the `navigator.credentials.get`
/// result).
#[derive(Debug, Clone)]
pub struct AuthAssertion {
    /// The credential id used.
    pub credential_id: Vec<u8>,
    /// Raw authenticator data (`rpIdHash ‖ flags ‖ signCount ‖ …`).
    pub authenticator_data: Vec<u8>,
    /// Raw client-data JSON bytes, hashed into the signed payload.
    pub client_data_json: Vec<u8>,
    /// The same client data, parsed by the caller.
    pub client_data: ClientData,
    /// Signature over `authenticatorData ‖ SHA-256(clientDataJSON)`.
    pub signature: Signature,
}

/// Why an assertion was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebAuthnError {
    /// The assertion's credential id does not match the registered passkey.
    WrongCredential,
    /// The client data was not a `webauthn.get`, or the authenticator data was short.
    BadClientData,
    /// The client-data challenge does not match the server challenge.
    ChallengeMismatch,
    /// The client-data origin is not one the policy allows.
    OriginMismatch,
    /// The authenticator-data RP-ID hash does not match the credential's RP id.
    RpMismatch,
    /// The user-present (UP) flag was not set.
    UserNotPresent,
    /// The policy requires user verification and the UV flag was not set.
    UserNotVerified,
    /// The signature did not verify under the passkey.
    BadSignature,
    /// The signature counter did not move past the last one accepted: the
    /// authenticator may have been cloned.
    CounterRegression,
}

/// The user-present flag bit in authenticator-data flags (byte 32).
const FLAG_USER_PRESENT: u8 = 0x01;
/// The user-verified flag bit in authenticator-data flags (byte 32).
const FLAG_USER_VERIFIED: u8 = 0x04;

impl WebAuthnCredential {
    /// Build authenticator data for this RP with the given flags + sign count (used
    /// by tests and by a local authenticator implementation).
    pub fn authenticator_data(&self, flags: u8, sign_count: u32) -> Vec<u8> {
        let mut ad = Vec::with_capacity(37);
        ad.extend_from_slice(&sha256(self.rp_id.as_bytes()));
        ad.push(flags);
        ad.extend_from_slice(&sign_count.to_be_bytes());
        ad
    }

    /// Canonical client-data JSON for a `get` assertion binding `challenge`.
    pub fn client_data_json(challenge: &[u8], origin: &str) -> Vec<u8> {
        format!(
            "{{\"type\":\"webauthn.get\",\"challenge\":\"{}\",\"origin\":\"{}\"}}",
            b64url(challenge),
            origin
        )
        .into_bytes()
    }

    /// The bytes an authenticator signs: `authenticatorData ‖ SHA-256(clientDataJSON)`.
    pub fn signed_payload(authenticator_data: &[u8], client_data_json: &[u8]) -> Vec<u8> {
        let mut p = authenticator_data.to_vec();
        p.extend_from_slice(&sha256(client_data_json));
        p
    }

    /// Verify an authentication assertion against a server-issued `challenge` under
    /// `policy`, and return the signature counter it presents, which the caller
    /// persists as this credential's `sign_count` before the login completes. Checks,
    /// in order: credential id, client-data type, challenge, origin, RP-ID hash, user
    /// presence, user verification (if the policy requires it), signature, counter.
    /// The counter check is skipped only when both the stored and the presented
    /// counter are 0 (an authenticator that keeps none). [RC-29] [RC-30]
    /// [VLD-ANB-MIDGARD-0084]
    pub fn verify_assertion(
        &self,
        provider: &CryptoProvider,
        policy: &WebAuthnPolicy,
        challenge: &[u8],
        assertion: &AuthAssertion,
    ) -> Result<u32, WebAuthnError> {
        if assertion.credential_id != self.credential_id {
            return Err(WebAuthnError::WrongCredential);
        }
        // 1. client data: type, challenge binding and origin.
        let cd = &assertion.client_data;
        if cd.type_ != "webauthn.get" {
            return Err(WebAuthnError::BadClientData);
        }
        if !ct_eq(&cd.challenge, challenge) {
            return Err(WebAuthnError::ChallengeMismatch);
        }
        if !policy.origins.iter().any(|o| *o == cd.origin) {
            return Err(WebAuthnError::OriginMismatch);
        }
        // 2. authenticator data: RP-ID hash, user-present and user-verified flags.
        let ad = &assertion.authenticator_data;
        if ad.len() < 37 {
            return Err(WebAuthnError::BadClientData);
        }
        let rp_hash = sha256(self.rp_id.as_bytes());
        if !ct_eq(&ad[..32], &rp_hash) {
            return Err(WebAuthnError::RpMismatch);
        }
        if ad[32] & FLAG_USER_PRESENT == 0 {
            return Err(WebAuthnError::UserNotPresent);
        }
        if policy.require_uv && ad[32] & FLAG_USER_VERIFIED == 0 {
            return Err(WebAuthnError::UserNotVerified);
        }
        // 3. signature over authenticatorData ‖ SHA-256(clientDataJSON).
        let payload = Self::signed_payload(ad, &assertion.client_data_json);
        let scheme = provider
            .sig_for(self.public_key.alg())
            .map_err(|_| WebAuthnError::BadSignature)?;
        scheme
            .verify(&self.public_key, &payload, &assertion.signature)
            .map_err(|_| WebAuthnError::BadSignature)?;
        // 4. signature counter: it must move forward, unless the authenticator keeps
        // none (both 0).
        let presented = u32::from_be_bytes([ad[33], ad[34], ad[35], ad[36]]);
        if (presented != 0 || self.sign_count != 0) && presented <= self.sign_count {
            return Err(WebAuthnError::CounterRegression);
        }
        Ok(presented)
    }
}

/// The passkey signature algorithm used by this enclosure's authenticators.
pub const PASSKEY_SIG_ALG: SigId = SigId::MlDsa65;
