//! Real WebAuthn (passkey) assertion verification for admin sign-in and step-up —
//! the same challenge/assertion gate the client keystore uses ([FR-C-9]/[AC-C-9]),
//! applied to the admin control panel. A relying party issues a random
//! [`AdminChallenge`]; the authenticator answers it *inside its secure boundary* by
//! signing the nonce with a credential key that has **no accessor**; the gate
//! ([`verify_admin_assertion`]) verifies the signature against the registered public
//! passkey. There is no way to produce a valid assertion without the credential
//! secret, so an admin session cannot be forged. [RC-30]

use pqc_agility::{CryptoProvider, Rng, SigId, Signature, VerifyingKey};

use crate::error::IdentityError;

/// The passkey signature algorithm (real ML-DSA-65 / FIPS 204).
pub const PASSKEY_SIG_ALG: SigId = SigId::MlDsa65;

/// A random challenge the relying party asks the authenticator to sign. The nonce
/// binds an assertion to this one attempt (anti-replay). [RC-30]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminChallenge {
    /// 32-byte random challenge.
    pub nonce: [u8; 32],
}

impl AdminChallenge {
    /// A challenge over a fixed nonce.
    pub fn new(nonce: [u8; 32]) -> Self {
        Self { nonce }
    }
    /// A freshly-sampled challenge.
    pub fn sample(rng: &mut dyn Rng) -> Self {
        let mut nonce = [0u8; 32];
        rng.fill(&mut nonce);
        Self { nonce }
    }
}

/// The public half of an enrolled passkey — safe to store. Carries no secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredPasskey {
    /// Stable credential id.
    pub credential_id: String,
    /// The passkey's public key (verifies assertions).
    pub public_key: VerifyingKey,
}

/// The proof an admin authenticated: a signature over the challenge nonce **and**
/// the user-verification flag. Contains no secret. A `user_verified` assertion means
/// the authenticator verified the user locally (biometric / PIN) — so a passkey with
/// user verification is itself two factors (possession + inherence/knowledge), the
/// FIDO2 notion of MFA. The flag is inside the signed payload, so it cannot be
/// flipped in transit. [RC-30]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAuthnAssertion {
    /// The credential that produced it.
    pub credential_id: String,
    /// Whether the authenticator verified the user (UV flag).
    pub user_verified: bool,
    /// Signature over `nonce ‖ user_verified`.
    pub signature: Signature,
}

/// The bytes an authenticator signs: the challenge nonce followed by the UV flag.
fn signed_payload(nonce: &[u8; 32], user_verified: bool) -> Vec<u8> {
    let mut p = Vec::with_capacity(33);
    p.extend_from_slice(nonce);
    p.push(user_verified as u8);
    p
}

/// Verify a WebAuthn assertion against a registered passkey and the challenge that
/// was issued. `Ok(())` iff the signature (over the nonce + UV flag) verifies; fails
/// closed otherwise. This is the sole gate through which an
/// [`AdminSession`](crate::AdminSession) is authenticated. Whether `user_verified`
/// is *required* is the caller's policy (sign-in requires it). [RC-30]
pub fn verify_admin_assertion(
    credential: &RegisteredPasskey,
    challenge: &AdminChallenge,
    assertion: &WebAuthnAssertion,
    provider: &CryptoProvider,
) -> Result<(), IdentityError> {
    if assertion.credential_id != credential.credential_id {
        return Err(IdentityError::WebAuthnRejected);
    }
    let scheme = provider
        .sig_for(credential.public_key.alg())
        .map_err(|_| IdentityError::WebAuthnRejected)?;
    let payload = signed_payload(&challenge.nonce, assertion.user_verified);
    scheme
        .verify(&credential.public_key, &payload, &assertion.signature)
        .map_err(|_| IdentityError::WebAuthnRejected)
}

/// A deterministic in-boundary authenticator for dev/tests (mirrors the keystore's
/// `SoftwareAuthenticator`). It holds a passkey keypair whose secret **never leaves**
/// — there is no accessor — exactly as a Secure-Enclave/WebAuthn credential exposes
/// none. A real authenticator (platform WebAuthn) drops in with the secret sealed in
/// hardware. [RC-30]
pub struct SoftwareAdminAuthenticator {
    credential: RegisteredPasskey,
    // Private, no accessor: the secret never crosses the API boundary.
    signing_key: pqc_agility::SigningKey,
}

impl SoftwareAdminAuthenticator {
    /// Enrol a fresh passkey credential signing with `PASSKEY_SIG_ALG`.
    pub fn enrol(
        provider: &CryptoProvider,
        credential_id: &str,
        rng: &mut dyn Rng,
    ) -> Result<Self, IdentityError> {
        let scheme = provider
            .sig_for(PASSKEY_SIG_ALG)
            .map_err(|_| IdentityError::WebAuthnRejected)?;
        let (public_key, signing_key) = scheme
            .keypair(rng)
            .map_err(|_| IdentityError::WebAuthnRejected)?;
        Ok(Self {
            credential: RegisteredPasskey {
                credential_id: credential_id.to_string(),
                public_key,
            },
            signing_key,
        })
    }

    /// Rebuild an authenticator from a passkey keypair held at rest — an admin's
    /// sealed keystore — so the same credential answers challenges across runs of the
    /// admin tool. Fails closed if the key is not [`PASSKEY_SIG_ALG`] or the two halves
    /// do not belong together (a probe assertion must verify). [RC-30]
    pub fn from_key(
        provider: &CryptoProvider,
        credential_id: &str,
        public_key: VerifyingKey,
        signing_key: pqc_agility::SigningKey,
    ) -> Result<Self, IdentityError> {
        if public_key.alg() != PASSKEY_SIG_ALG || signing_key.alg() != PASSKEY_SIG_ALG {
            return Err(IdentityError::WebAuthnRejected);
        }
        let authenticator = Self {
            credential: RegisteredPasskey {
                credential_id: credential_id.to_string(),
                public_key,
            },
            signing_key,
        };
        let probe = AdminChallenge::new([0x5a; 32]);
        let assertion = authenticator.authenticate(provider, &probe, true)?;
        verify_admin_assertion(&authenticator.credential, &probe, &assertion, provider)?;
        Ok(authenticator)
    }

    /// The public passkey (for registration / verification).
    pub fn credential(&self) -> &RegisteredPasskey {
        &self.credential
    }

    /// Answer a challenge inside the secure boundary, returning an assertion.
    /// `user_verified` reflects whether the local biometric/PIN gate passed.
    pub fn authenticate(
        &self,
        provider: &CryptoProvider,
        challenge: &AdminChallenge,
        user_verified: bool,
    ) -> Result<WebAuthnAssertion, IdentityError> {
        let scheme = provider
            .sig_for(self.signing_key.alg())
            .map_err(|_| IdentityError::WebAuthnRejected)?;
        let payload = signed_payload(&challenge.nonce, user_verified);
        let signature = scheme
            .sign(&self.signing_key, &payload)
            .map_err(|_| IdentityError::WebAuthnRejected)?;
        Ok(WebAuthnAssertion {
            credential_id: self.credential.credential_id.clone(),
            user_verified,
            signature,
        })
    }
}
