//! Multi-factor authentication: **TOTP** (RFC 6238) and **WebAuthn/passkey** [NFR-SEC-2]
//! assertion verification. Both are second factors checked *before* a token is
//! minted. [RC-29]
//!
//! ## TOTP
//! HOTP (RFC 4226) over HMAC-SHA256 with a 30-second time step and ±1-step skew.
//! The shared secret is provisioned once and base32-encoded into an `otpauth://`
//! URI for an authenticator app.
//!
//! ## WebAuthn
//! A passkey here is an **ML-DSA** credential (the enclosure's PQ signature scheme
//! in place of COSE/ES256). `verify_assertion` performs the security-relevant
//! WebAuthn checks: the client-data challenge binds to the server challenge, the
//! authenticator-data RP-ID hash matches, the user-present flag is set, and the
//! signature over `authenticatorData ‖ SHA-256(clientDataJSON)` verifies. Parsing of
//! the COSE public key, CBOR attestation object, and registration ceremony is out of
//! scope (documented) — the assertion (login) path is real. [RC-29] [RC-30]

use pqc_agility::hash::{ct_eq, hmac_sha256, sha256};
use pqc_agility::{CryptoProvider, SigId, Signature, VerifyingKey};

use crate::enc::{b32, b64url, unb64url};

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
    /// Constant-time comparison. [RC-29]
    pub fn verify(&self, code: &str, unix_time: u64) -> bool {
        let counter = unix_time / TIME_STEP;
        for step in [counter.wrapping_sub(1), counter, counter + 1] {
            let expected = hotp(&self.secret, step);
            if ct_eq(expected.as_bytes(), code.as_bytes()) {
                return true;
            }
        }
        false
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
}

/// An authentication assertion presented at login (the `navigator.credentials.get`
/// result).
#[derive(Debug, Clone)]
pub struct AuthAssertion {
    /// The credential id used.
    pub credential_id: Vec<u8>,
    /// Raw authenticator data (`rpIdHash ‖ flags ‖ signCount ‖ …`).
    pub authenticator_data: Vec<u8>,
    /// Raw client-data JSON bytes.
    pub client_data_json: Vec<u8>,
    /// Signature over `authenticatorData ‖ SHA-256(clientDataJSON)`.
    pub signature: Signature,
}

/// Why an assertion was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebAuthnError {
    /// The assertion's credential id does not match the registered passkey.
    WrongCredential,
    /// The client-data JSON was malformed or not a `webauthn.get`.
    BadClientData,
    /// The client-data challenge does not match the server challenge.
    ChallengeMismatch,
    /// The authenticator-data RP-ID hash does not match the credential's RP id.
    RpMismatch,
    /// The user-present (UP) flag was not set.
    UserNotPresent,
    /// The signature did not verify under the passkey.
    BadSignature,
}

/// The user-present flag bit in authenticator-data flags (byte 32).
const FLAG_USER_PRESENT: u8 = 0x01;

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

    /// Verify an authentication assertion against a server-issued `challenge`.
    /// Performs the WebAuthn login-ceremony security checks. [RC-29] [RC-30]
    pub fn verify_assertion(
        &self,
        provider: &CryptoProvider,
        challenge: &[u8],
        assertion: &AuthAssertion,
    ) -> Result<(), WebAuthnError> {
        if assertion.credential_id != self.credential_id {
            return Err(WebAuthnError::WrongCredential);
        }
        // 1. client-data JSON: type + challenge binding.
        let cd = std::str::from_utf8(&assertion.client_data_json)
            .map_err(|_| WebAuthnError::BadClientData)?;
        if !cd.contains("\"type\":\"webauthn.get\"") {
            return Err(WebAuthnError::BadClientData);
        }
        let got_challenge =
            extract_json_string(cd, "challenge").ok_or(WebAuthnError::BadClientData)?;
        let got = unb64url(&got_challenge).ok_or(WebAuthnError::BadClientData)?;
        if !ct_eq(&got, challenge) {
            return Err(WebAuthnError::ChallengeMismatch);
        }
        // 2. authenticator data: RP-ID hash + user-present flag.
        if assertion.authenticator_data.len() < 37 {
            return Err(WebAuthnError::BadClientData);
        }
        let rp_hash = sha256(self.rp_id.as_bytes());
        if !ct_eq(&assertion.authenticator_data[..32], &rp_hash) {
            return Err(WebAuthnError::RpMismatch);
        }
        if assertion.authenticator_data[32] & FLAG_USER_PRESENT == 0 {
            return Err(WebAuthnError::UserNotPresent);
        }
        // 3. signature over authenticatorData ‖ SHA-256(clientDataJSON).
        let payload =
            Self::signed_payload(&assertion.authenticator_data, &assertion.client_data_json);
        let scheme = provider
            .sig_for(self.public_key.alg())
            .map_err(|_| WebAuthnError::BadSignature)?;
        scheme
            .verify(&self.public_key, &payload, &assertion.signature)
            .map_err(|_| WebAuthnError::BadSignature)?;
        Ok(())
    }
}

/// The passkey signature algorithm used by this enclosure's authenticators.
pub const PASSKEY_SIG_ALG: SigId = SigId::MlDsa65;

/// Extract a JSON string value for `key` from a flat object (no nesting/escapes —
/// sufficient for the fixed client-data shape).
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
