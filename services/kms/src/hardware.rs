//! Hardware trust roots: a KMS root whose ML-KEM-1024 secret **lives inside a
//! device** (TPM 2.0 / PKCS#11 HSM) and never enters this process's memory. The KMS
//! sends the device a wrapped-key ciphertext and receives back the shared secret;
//! the private key is non-exportable and stays on the token. [NFR-PQ-2] [RC-28]
//!
//! ## The device seam
//! [`HardwareToken`] is the driver interface a concrete device implements:
//! - **PKCS#11 HSM:** `decapsulate` → `C_DecapsulateKey`/`C_Decrypt` on a
//!   `CKA_EXTRACTABLE=false` key object; `attest` → the token's key-attestation
//!   attributes signed by the HSM's attestation key.
//! - **TPM 2.0:** `decapsulate` → `TPM2_ECDH_ZGen`/a PQ decaps command on a key
//!   whose `fixedTPM` + `fixedParent` bits are set; `attest` → a `TPM2_Certify`
//!   quote over the key's public area and name.
//!
//! [`HardwareTrustRoot`] wraps any `HardwareToken` and implements the crate's
//! [`TrustRoot`](crate::TrustRoot) trait by **delegating unwrap to the device** —
//! it holds no key bytes itself.
//!
//! ## What ships here
//! [`SoftHsmToken`] is a **software simulation of the device boundary** (a SoftHSM
//! stand-in) so the architecture — delegation, attestation, and "the KMS process
//! never holds the root secret" — is testable offline. A production build replaces
//! it with a real PKCS#11 or TPM 2.0 `HardwareToken`; nothing above the seam changes.
//! The residual risk of launching with the software root instead is documented in
//! `services/kms/RESIDUAL_RISK.md`.

use std::sync::atomic::{AtomicU64, Ordering};

use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::hash::sha256;
use pqc_agility::{
    Ciphertext, CryptoProvider, Kem, KemPublicKey, KemSecretKey, Rng, SharedSecret, SigId,
    Signature, VerifyingKey,
};

use crate::error::KmsError;
use crate::{TrustRoot, WRAP_SUITE};

/// The signature scheme a device uses to sign its root attestation.
pub const ATTEST_SIG_ALG: SigId = SigId::MlDsa65;

/// A device-signed statement that a KMS root key is resident on the device and
/// **non-exportable**. A verifier checks the signature under the device's
/// attestation key and that the quote binds the expected root key id + public key.
/// [RC-28]
#[derive(Debug, Clone)]
pub struct RootAttestation {
    /// The attesting device's identifier.
    pub device_id: String,
    /// The root key id being attested.
    pub key_id: String,
    /// SHA-256 of the root's ML-KEM public key (binds the quote to a specific key).
    pub public_key_digest: [u8; 32],
    /// Whether the device asserts the private key cannot be exported. A real
    /// attestation only sets this from a hardware non-extractable attribute.
    pub non_exportable: bool,
    /// Algorithm of `signature`.
    pub sig_alg: SigId,
    /// The device's signature over the canonical quote bytes.
    pub signature: Signature,
}

impl RootAttestation {
    /// The canonical bytes signed by the device.
    fn canonical(
        device_id: &str,
        key_id: &str,
        pk_digest: &[u8; 32],
        non_exportable: bool,
    ) -> Vec<u8> {
        let mut o = Vec::new();
        let mut put = |b: &[u8]| {
            o.extend_from_slice(&(b.len() as u32).to_le_bytes());
            o.extend_from_slice(b);
        };
        put(device_id.as_bytes());
        put(key_id.as_bytes());
        put(pk_digest);
        o.push(non_exportable as u8);
        o
    }

    fn signed_bytes(&self) -> Vec<u8> {
        Self::canonical(
            &self.device_id,
            &self.key_id,
            &self.public_key_digest,
            self.non_exportable,
        )
    }

    /// Verify the attestation's signature under the device attestation key, and that
    /// it asserts non-exportability. Fails closed. [RC-28]
    pub fn verify(
        &self,
        provider: &CryptoProvider,
        attestation_key: &VerifyingKey,
    ) -> Result<(), KmsError> {
        let scheme = provider
            .sig_for(self.sig_alg)
            .map_err(|_| KmsError::AttestationInvalid)?;
        scheme
            .verify(attestation_key, &self.signed_bytes(), &self.signature)
            .map_err(|_| KmsError::AttestationInvalid)?;
        if !self.non_exportable {
            return Err(KmsError::AttestationInvalid);
        }
        Ok(())
    }

    /// Whether this attestation binds the given root key id and public key.
    pub fn binds(&self, key_id: &str, public_key: &KemPublicKey) -> bool {
        self.key_id == key_id && self.public_key_digest == sha256(public_key.as_bytes())
    }
}

/// A key-custody device (TPM 2.0 / PKCS#11 HSM). The root secret lives inside it and
/// is never returned; the device performs decapsulation on-chip. Implementors are
/// concrete drivers; [`SoftHsmToken`] is the offline simulation. [RC-28]
pub trait HardwareToken: Send + Sync {
    /// The device-resident root key id.
    fn key_id(&self) -> &str;
    /// The root's ML-KEM public key (safe to export).
    fn public_key(&self) -> &KemPublicKey;
    /// Decapsulate `wrapped` **on the device**, returning only the shared secret.
    /// The private key never leaves the token.
    fn decapsulate(&self, wrapped: &Ciphertext) -> Result<SharedSecret, KmsError>;
    /// A fresh device attestation quote for the resident root key.
    fn attest(&self) -> RootAttestation;
    /// The device's attestation verifying key (a verifier's trust anchor for the
    /// quote; provisioned out of band from the device's endorsement hierarchy).
    fn attestation_key(&self) -> &VerifyingKey;
}

/// A trust root backed by a [`HardwareToken`]. It holds **only** the token handle —
/// no secret-key bytes exist in this struct or anywhere in the KMS process. Unwrap
/// is delegated to the device. [RC-28]
pub struct HardwareTrustRoot {
    token: Box<dyn HardwareToken>,
}

impl HardwareTrustRoot {
    /// Adopt a device token as the trust root.
    pub fn new(token: Box<dyn HardwareToken>) -> Self {
        Self { token }
    }

    /// The device's current root attestation.
    pub fn attest(&self) -> RootAttestation {
        self.token.attest()
    }

    /// Verify the device's attestation: the quote is signed by the device's
    /// attestation key, asserts a non-exportable key, and binds this root's id +
    /// public key. Operators run this at provisioning before trusting the root.
    /// [RC-28]
    pub fn verify_attestation(&self, provider: &CryptoProvider) -> Result<(), KmsError> {
        let att = self.token.attest();
        att.verify(provider, self.token.attestation_key())?;
        if !att.binds(self.token.key_id(), self.token.public_key()) {
            return Err(KmsError::AttestationInvalid);
        }
        Ok(())
    }
}

impl TrustRoot for HardwareTrustRoot {
    fn key_id(&self) -> &str {
        self.token.key_id()
    }
    fn public_key(&self) -> &KemPublicKey {
        self.token.public_key()
    }
    fn unwrap(&self, _kem: &dyn Kem, wrapped: &Ciphertext) -> Result<SharedSecret, KmsError> {
        // Decapsulation happens ON the device. The software `kem` argument is
        // deliberately ignored: this process never holds the root secret and never
        // performs the decapsulation itself. [RC-28]
        self.token.decapsulate(wrapped)
    }
}

/// A **software simulation** of a key-custody device — a SoftHSM stand-in for
/// offline development and CI. The root secret and attestation key live inside this
/// struct, modelling the device's internal secure memory; the KMS above the
/// [`HardwareToken`] seam still never touches them. A production build swaps this for
/// a real PKCS#11 / TPM 2.0 driver, where the secret is physically device-bound.
/// [RC-28]
pub struct SoftHsmToken {
    device_id: String,
    key_id: String,
    public_key: KemPublicKey,
    // ---- inside the simulated device boundary (never exposed) ----
    provider: CryptoProvider,
    secret_key: KemSecretKey,
    attestation: RootAttestation,
    attestation_key: VerifyingKey,
    // On-device operation counter, to demonstrate delegation in tests.
    decaps: AtomicU64,
}

impl SoftHsmToken {
    /// Provision a fresh simulated device: generate the resident ML-KEM-1024 root
    /// keypair and a device attestation keypair, and self-issue the attestation.
    pub fn provision(device_id: &str, key_id: &str, rng: &mut dyn Rng) -> Result<Self, KmsError> {
        let provider = fips_provider();
        let kem = provider
            .kem_for(WRAP_SUITE)
            .map_err(|_| KmsError::SuiteUnavailable)?;
        let (public_key, secret_key) =
            kem.keypair(rng).map_err(|e| KmsError::Kem(e.to_string()))?;

        let sig = provider
            .sig_for(ATTEST_SIG_ALG)
            .map_err(|_| KmsError::HardwareUnavailable("attestation scheme".into()))?;
        let (attestation_key, attest_sk) = sig
            .keypair(rng)
            .map_err(|e| KmsError::HardwareUnavailable(e.to_string()))?;

        let public_key_digest = sha256(public_key.as_bytes());
        let signed = RootAttestation::canonical(device_id, key_id, &public_key_digest, true);
        let signature = sig
            .sign(&attest_sk, &signed)
            .map_err(|e| KmsError::HardwareUnavailable(e.to_string()))?;
        let attestation = RootAttestation {
            device_id: device_id.to_string(),
            key_id: key_id.to_string(),
            public_key_digest,
            non_exportable: true,
            sig_alg: ATTEST_SIG_ALG,
            signature,
        };
        // `attest_sk` is dropped here: like a device endorsement key, it is not held
        // for reuse beyond issuing the quote.
        let _ = attest_sk;

        Ok(Self {
            device_id: device_id.to_string(),
            key_id: key_id.to_string(),
            public_key,
            provider,
            secret_key,
            attestation,
            attestation_key,
            decaps: AtomicU64::new(0),
        })
    }

    /// How many on-device decapsulations this token has performed (test/telemetry
    /// hook that shows unwrap is delegated to the device).
    pub fn decapsulation_count(&self) -> u64 {
        self.decaps.load(Ordering::Relaxed)
    }
}

impl HardwareToken for SoftHsmToken {
    fn key_id(&self) -> &str {
        &self.key_id
    }
    fn public_key(&self) -> &KemPublicKey {
        &self.public_key
    }
    fn decapsulate(&self, wrapped: &Ciphertext) -> Result<SharedSecret, KmsError> {
        // On-device operation: the secret key never leaves this boundary.
        self.decaps.fetch_add(1, Ordering::Relaxed);
        let kem = self
            .provider
            .kem_for(WRAP_SUITE)
            .map_err(|_| KmsError::SuiteUnavailable)?;
        kem.decapsulate(&self.secret_key, wrapped)
            .map_err(|e| KmsError::Kem(e.to_string()))
    }
    fn attest(&self) -> RootAttestation {
        self.attestation.clone()
    }
    fn attestation_key(&self) -> &VerifyingKey {
        &self.attestation_key
    }
}

/// `device_id` accessor for diagnostics (kept off the trait to stay minimal).
impl SoftHsmToken {
    /// The simulated device id.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }
}
