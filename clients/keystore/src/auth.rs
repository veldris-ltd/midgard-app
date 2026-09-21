//! Platform authentication: Face ID / Touch ID / Windows Hello / Android
//! BiometricPrompt / WebAuthn. `[FR-C-9]` `[AC-C-9]`
//!
//! The invariant: **no secret leaves the secure boundary.** A [`PlatformAuthenticator`]
//! answers a challenge *inside* custody by signing it with a credential key that has
//! no accessor — the only thing that crosses the API is an [`AuthAssertion`] (a
//! signature over the challenge), never the key or the biometric. The gate
//! ([`verify_assertion`]) checks the assertion against the registered *public*
//! credential; on success the caller may unlock the keystore. This is the WebAuthn
//! model, applied uniformly across the native biometrics.

use pqc_agility::{CryptoProvider, Rng, SigId, Signature, VerifyingKey};

use crate::error::KeystoreError;
use crate::PlatformProfile;

/// The local authentication mechanism a platform offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMechanism {
    /// Apple Face ID.
    FaceId,
    /// Apple Touch ID.
    TouchId,
    /// Windows Hello.
    WindowsHello,
    /// Android BiometricPrompt.
    AndroidBiometricPrompt,
    /// Web Authentication (browser).
    WebAuthn,
}

impl AuthMechanism {
    /// The default mechanism for a platform. (Apple defaults to Face ID; a shell may
    /// fall back to Touch ID at runtime.)
    pub fn for_platform(profile: PlatformProfile) -> Self {
        match profile {
            PlatformProfile::Apple => AuthMechanism::FaceId,
            PlatformProfile::Android => AuthMechanism::AndroidBiometricPrompt,
            PlatformProfile::Windows => AuthMechanism::WindowsHello,
            PlatformProfile::Linux => AuthMechanism::WebAuthn,
            PlatformProfile::WebApp | PlatformProfile::Extension => AuthMechanism::WebAuthn,
        }
    }
}

/// A random challenge the relying party asks the authenticator to sign. The nonce
/// binds the assertion to this attempt (anti-replay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthChallenge {
    /// 32-byte random challenge.
    pub nonce: [u8; 32],
}

impl AuthChallenge {
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

/// The public half of an enrolled credential — safe to store and share. Carries no
/// secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredCredential {
    /// Stable credential id.
    pub credential_id: String,
    /// The credential's public key (verifies assertions).
    pub public_key: VerifyingKey,
    /// The mechanism that produced it.
    pub mechanism: AuthMechanism,
}

/// The proof a user authenticated: a signature over the challenge. **Contains no
/// secret** — only what a relying party needs to verify. `[AC-C-9]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthAssertion {
    /// The credential that produced it.
    pub credential_id: String,
    /// The mechanism used.
    pub mechanism: AuthMechanism,
    /// Signature over the challenge nonce.
    pub signature: Signature,
}

/// Authenticates the user locally. The credential secret lives inside the boundary;
/// `authenticate` is the only operation, and it emits an assertion, never a key.
pub trait PlatformAuthenticator {
    /// The mechanism this authenticator uses.
    fn mechanism(&self) -> AuthMechanism;
    /// The public credential (for enrolment / verification).
    fn credential(&self) -> &RegisteredCredential;
    /// Answer a challenge inside the secure boundary, returning an assertion.
    fn authenticate(
        &self,
        provider: &CryptoProvider,
        challenge: &AuthChallenge,
    ) -> Result<AuthAssertion, KeystoreError>;
}

/// Verify an assertion against a registered credential and the challenge that was
/// issued. `Ok(())` iff the user authenticated; fails closed otherwise — the sole
/// gate that unlocks the keystore. `[AC-C-9]`
pub fn verify_assertion(
    credential: &RegisteredCredential,
    challenge: &AuthChallenge,
    assertion: &AuthAssertion,
    provider: &CryptoProvider,
) -> Result<(), KeystoreError> {
    if assertion.credential_id != credential.credential_id {
        return Err(KeystoreError::AuthDenied);
    }
    let scheme = provider
        .sig_for(credential.public_key.alg())
        .map_err(|_| KeystoreError::AuthDenied)?;
    scheme
        .verify(
            &credential.public_key,
            &challenge.nonce,
            &assertion.signature,
        )
        .map_err(|_| KeystoreError::AuthDenied)
}

/// A deterministic in-boundary authenticator for dev/tests. It holds a credential
/// keypair whose secret **never leaves** — there is no accessor for it, exactly as
/// a Secure-Enclave/WebAuthn credential exposes none. A real authenticator drops in
/// with the secret sealed in hardware.
pub struct SoftwareAuthenticator {
    credential: RegisteredCredential,
    // Private, no accessor: the secret never crosses the API boundary. [AC-C-9]
    signing_key: pqc_agility::SigningKey,
}

impl SoftwareAuthenticator {
    /// Enrol a fresh credential of `mechanism`, signing with `alg`.
    pub fn enrol(
        provider: &CryptoProvider,
        credential_id: &str,
        mechanism: AuthMechanism,
        alg: SigId,
        rng: &mut dyn Rng,
    ) -> Result<Self, KeystoreError> {
        let scheme = provider
            .sig_for(alg)
            .map_err(|_| KeystoreError::AuthBackendUnavailable)?;
        let (vk, sk) = scheme
            .keypair(rng)
            .map_err(|e| KeystoreError::Enrolment(e.to_string()))?;
        Ok(Self {
            credential: RegisteredCredential {
                credential_id: credential_id.to_string(),
                public_key: vk,
                mechanism,
            },
            signing_key: sk,
        })
    }
}

impl PlatformAuthenticator for SoftwareAuthenticator {
    fn mechanism(&self) -> AuthMechanism {
        self.credential.mechanism
    }
    fn credential(&self) -> &RegisteredCredential {
        &self.credential
    }
    fn authenticate(
        &self,
        provider: &CryptoProvider,
        challenge: &AuthChallenge,
    ) -> Result<AuthAssertion, KeystoreError> {
        let scheme = provider
            .sig_for(self.signing_key.alg())
            .map_err(|_| KeystoreError::AuthBackendUnavailable)?;
        let signature = scheme
            .sign(&self.signing_key, &challenge.nonce)
            .map_err(|e| KeystoreError::AuthFailed(e.to_string()))?;
        Ok(AuthAssertion {
            credential_id: self.credential.credential_id.clone(),
            mechanism: self.credential.mechanism,
            signature,
        })
    }
}
