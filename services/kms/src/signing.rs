//! # Signing custody — the signature counterpart to [`crate::TrustRoot`]
//!
//! [`TrustRoot`](crate::TrustRoot) is the custody seam for *decapsulation*: the
//! root's KEM secret unwraps a data key, in software or inside an HSM/TPM, and
//! the secret itself is never returned. [`SigningRoot`] is the same seam for
//! *signatures*: the caller hands over a message and receives a signature; the
//! signing key never crosses the boundary. That is what lets a hardware
//! custodian be substituted for [`SoftwareSigningRoot`] without any caller
//! changing. `[NFR-PQ-3]` `[STD:OWASP ASVS V14.3]`
//!
//! Nothing here writes key material to disk. At-rest custody of a software
//! signing key is the caller's problem, and the suite's answer is the existing
//! KEM-DEM envelope ([`crate::Kms::seal`]) — see `attest`'s keystore, which
//! seals an ML-DSA key under an ML-KEM-1024 root and only ever reconstructs a
//! [`SoftwareSigningRoot`] in memory after unsealing through [`crate::TrustRoot`].
//!
//! ## Honest limitation
//! A [`SoftwareSigningRoot`] holds its secret in process memory while it lives,
//! so it is software-tier custody — equivalent to a software HSM, not to a
//! hardware one. The point of the trait is that upgrading that tier is a
//! substitution, not a rewrite.

use pqc_agility::{CryptoProvider, Rng, SigId, Signature, SigningKey, VerifyingKey};

use crate::error::KmsError;

/// The signing algorithm used for release artefacts: ML-DSA-87 (FIPS 204), the
/// strongest parameter set, because the release key is the highest-value key in
/// the enclosure. Per-artefact and model manifests sign at ML-DSA-65 — see
/// `bifrost_artefact::BIFROST_SIG_ALG` and
/// `model_server::artifact::VELDRIS_MODEL_SIG_ALG`. `[NFR-PQ-3]`
pub const RELEASE_SIG_ALG: SigId = SigId::MlDsa87;

/// Where a signing key lives and where signing happens.
///
/// Implementors sign **inside** the custodian. There is deliberately no method
/// that returns the signing key: a caller cannot exfiltrate it through this
/// interface, so the same code path is correct for a software key, a TPM, and an
/// HSM. `[NFR-PQ-3]`
pub trait SigningRoot {
    /// Stable identifier of this signing key, recorded in every signature
    /// envelope so a verifier can select the right trust anchor.
    fn key_id(&self) -> &str;

    /// The algorithm this root signs with.
    fn algorithm(&self) -> SigId;

    /// The public verifying key, for distribution as a trust anchor.
    fn verifying_key(&self) -> &VerifyingKey;

    /// Sign `msg`. Fails closed — there is no path that returns a placeholder
    /// or an unsigned result.
    fn sign(&self, msg: &[u8]) -> Result<Signature, KmsError>;
}

/// A software-held signing root: the ML-DSA keypair lives in this process.
///
/// The hardware seam is [`SigningRoot`] itself — a TPM/HSM implementation offers
/// the identical surface with the secret never leaving the device. Mirrors
/// [`crate::SoftwareTrustRoot`]'s shape exactly.
pub struct SoftwareSigningRoot {
    key_id: String,
    provider: CryptoProvider,
    verifying_key: VerifyingKey,
    signing_key: SigningKey,
}

impl SoftwareSigningRoot {
    /// Generate a fresh signing keypair for `alg`.
    ///
    /// Fails closed if `alg` is not registered and enabled in `provider` —
    /// an unavailable algorithm must never silently downgrade. `[AC-M-1]`
    pub fn generate(
        provider: CryptoProvider,
        key_id: &str,
        alg: SigId,
        rng: &mut dyn Rng,
    ) -> Result<Self, KmsError> {
        let (verifying_key, signing_key) = provider
            .sig_for(alg)
            .map_err(|e| KmsError::Sign(e.to_string()))?
            .keypair(rng)
            .map_err(|e| KmsError::Sign(e.to_string()))?;
        Ok(Self {
            key_id: key_id.to_string(),
            provider,
            verifying_key,
            signing_key,
        })
    }

    /// Reconstruct a signing root from key material the caller has already
    /// recovered — typically by unsealing it through [`crate::TrustRoot`].
    ///
    /// The two keys must agree on their algorithm; a mismatch is rejected here
    /// rather than surfacing later as a confusing verification failure.
    pub fn from_key(
        provider: CryptoProvider,
        key_id: &str,
        verifying_key: VerifyingKey,
        signing_key: SigningKey,
    ) -> Result<Self, KmsError> {
        if verifying_key.alg() != signing_key.alg() {
            return Err(KmsError::Sign(format!(
                "key algorithm mismatch: verifying {}, signing {}",
                verifying_key.alg().as_str(),
                signing_key.alg().as_str()
            )));
        }
        // The algorithm must be usable now, not merely nameable.
        provider
            .sig_for(signing_key.alg())
            .map_err(|e| KmsError::Sign(e.to_string()))?;
        Ok(Self {
            key_id: key_id.to_string(),
            provider,
            verifying_key,
            signing_key,
        })
    }

    /// Borrow the signing key so the caller can place it into custody at rest
    /// (seal it). This is the one deliberate escape hatch, and it is **not** on
    /// the [`SigningRoot`] trait — a custodian reached through the trait can
    /// never be asked for its key.
    pub fn expose_signing_key(&self) -> &SigningKey {
        &self.signing_key
    }
}

/// Hand-written so a signing root can appear in an error or a log line without
/// its key material following. Prints identity and algorithm only — never the
/// secret, and never the raw verifying-key bytes. `[NFR-PQ-3]`
impl core::fmt::Debug for SoftwareSigningRoot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SoftwareSigningRoot")
            .field("key_id", &self.key_id)
            .field("algorithm", &self.signing_key.alg().as_str())
            .finish_non_exhaustive()
    }
}

impl SigningRoot for SoftwareSigningRoot {
    fn key_id(&self) -> &str {
        &self.key_id
    }

    fn algorithm(&self) -> SigId {
        self.signing_key.alg()
    }

    fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    fn sign(&self, msg: &[u8]) -> Result<Signature, KmsError> {
        self.provider
            .sig_for(self.signing_key.alg())
            .map_err(|e| KmsError::Sign(e.to_string()))?
            .sign(&self.signing_key, msg)
            .map_err(|e| KmsError::Sign(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Needed to call `verify` on the concrete `FipsSig`; the production path
    // above goes through `&dyn SignatureScheme`, which needs no import.
    use pqc_agility::SignatureScheme;

    fn root(alg: SigId) -> SoftwareSigningRoot {
        let mut rng = pqc_agility::OsRng;
        SoftwareSigningRoot::generate(
            pqc_agility::backend::fips::provider(),
            "release-key-1",
            alg,
            &mut rng,
        )
        .expect("generate")
    }

    #[test]
    fn signs_and_verifies_through_the_seam() {
        let r = root(RELEASE_SIG_ALG);
        assert_eq!(r.key_id(), "release-key-1");
        assert_eq!(r.algorithm(), SigId::MlDsa87);

        let sig = r.sign(b"release payload").expect("sign");
        pqc_agility::backend::fips::FipsSig::new(RELEASE_SIG_ALG)
            .verify(r.verifying_key(), b"release payload", &sig)
            .expect("verifies under the exported verifying key");
    }

    #[test]
    fn signature_does_not_verify_over_a_different_message() {
        let r = root(RELEASE_SIG_ALG);
        let sig = r.sign(b"message A").expect("sign");
        assert!(
            pqc_agility::backend::fips::FipsSig::new(RELEASE_SIG_ALG)
                .verify(r.verifying_key(), b"message B", &sig)
                .is_err(),
            "a signature must not verify over other bytes"
        );
    }

    #[test]
    fn generate_fails_closed_on_a_disabled_algorithm() {
        let mut provider = pqc_agility::backend::fips::provider();
        provider.disable_sig(SigId::MlDsa87);
        let mut rng = pqc_agility::OsRng;
        let err = SoftwareSigningRoot::generate(provider, "k", SigId::MlDsa87, &mut rng)
            .expect_err("a disabled algorithm must not produce a key");
        assert!(matches!(err, KmsError::Sign(_)));
    }

    #[test]
    fn from_key_rejects_mismatched_key_algorithms() {
        let a = root(SigId::MlDsa87);
        let b = root(SigId::MlDsa65);
        // Take b's verifying key with a's signing key — an inconsistent pair.
        let err = SoftwareSigningRoot::from_key(
            pqc_agility::backend::fips::provider(),
            "k",
            b.verifying_key().clone(),
            pqc_agility::SigningKey::new(
                a.algorithm(),
                pqc_agility::SecretBytes::new(a.expose_signing_key().expose().to_vec()),
            ),
        )
        .expect_err("mismatched key algorithms must be rejected");
        assert!(matches!(err, KmsError::Sign(_)));
    }

    #[test]
    fn round_trips_through_from_key() {
        let r = root(RELEASE_SIG_ALG);
        let rebuilt = SoftwareSigningRoot::from_key(
            pqc_agility::backend::fips::provider(),
            r.key_id(),
            r.verifying_key().clone(),
            pqc_agility::SigningKey::new(
                r.algorithm(),
                pqc_agility::SecretBytes::new(r.expose_signing_key().expose().to_vec()),
            ),
        )
        .expect("rebuild");

        let sig = rebuilt.sign(b"payload").expect("sign");
        pqc_agility::backend::fips::FipsSig::new(RELEASE_SIG_ALG)
            .verify(r.verifying_key(), b"payload", &sig)
            .expect("a rebuilt root signs under the same identity");
    }
}
