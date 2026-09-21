//! Where the signing key lives.
//!
//! [`SigningBackend`] is the seam the signer talks to. Two implementations ship,
//! both behind the `custody` feature:
//!
//! - [`KmsSigner`] — wraps any [`kms::SigningRoot`], so signing happens inside
//!   the custodian and the key never crosses the boundary. A TPM or HSM root
//!   substitutes here with no change above this line. This is the production
//!   path. `[NFR-PQ-3]`
//! - [`FileSigner`] — a plaintext key on disk, **refused** unless
//!   `MIDGARD_ALLOW_FILE_SIGNING_KEY=1` is set. Local development only.
//!   `[STD:OWASP ASVS V14.3]`
//!
//! Verifiers need neither, which is why they are feature-gated: `bifrost-artefact`
//! and `model-server` depend on this crate with `default-features = false` and do
//! not pull the key-management service in merely to check a signature.
//!
//! ## No ambient key material
//! Nothing in this crate has a default key path, reads a key from an environment
//! variable, or searches the repository, the image, or the user's home directory.
//! A caller must name its key source explicitly, every time. The environment
//! variable above is an *opt-in to a weaker mode*, not a key location.

#[cfg(feature = "custody")]
use kms::SigningRoot;
#[cfg(feature = "custody")]
use pqc_agility::{CryptoProvider, SigId, SigningKey};
use pqc_agility::{Signature, VerifyingKey};

use crate::error::SignError;

/// The environment opt-in required before a plaintext key file may be used.
pub const FILE_SIGNING_ENV: &str = "MIDGARD_ALLOW_FILE_SIGNING_KEY";

/// Whether the plaintext-key opt-in is set. Exactly `"1"` — a stray truthy-looking
/// value must not enable a weaker mode by accident.
pub fn file_signing_permitted() -> bool {
    std::env::var(FILE_SIGNING_ENV)
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// A source of release signatures.
///
/// The trait is always available so callers can hold `&dyn SigningBackend`; the
/// concrete custodians below need the `custody` feature.
pub trait SigningBackend {
    /// The signing key's stable id, recorded in the envelope.
    fn key_id(&self) -> &str;
    /// Sign `msg`, or fail closed.
    fn sign(&self, msg: &[u8]) -> Result<Signature, SignError>;
    /// The public key a verifier needs, for export as a trust anchor.
    fn verifying_key(&self) -> &VerifyingKey;
}

/// Signs through a KMS/HSM custodian. The signing key is never returned to this
/// process by the [`kms::SigningRoot`] interface.
#[cfg(feature = "custody")]
pub struct KmsSigner<R: SigningRoot> {
    root: R,
}

#[cfg(feature = "custody")]
impl<R: SigningRoot> KmsSigner<R> {
    /// Wrap a signing root.
    pub fn new(root: R) -> Self {
        Self { root }
    }

    /// The algorithm the custodian signs with.
    pub fn algorithm(&self) -> SigId {
        self.root.algorithm()
    }
}

#[cfg(feature = "custody")]
impl<R: SigningRoot> SigningBackend for KmsSigner<R> {
    fn key_id(&self) -> &str {
        self.root.key_id()
    }

    fn sign(&self, msg: &[u8]) -> Result<Signature, SignError> {
        self.root
            .sign(msg)
            .map_err(|e| SignError::Custodian(e.to_string()))
    }

    fn verifying_key(&self) -> &VerifyingKey {
        self.root.verifying_key()
    }
}

/// A plaintext signing key held in this process, for local development.
///
/// Construction is gated on [`FILE_SIGNING_ENV`]; there is no way to build one
/// without the operator having explicitly opted in.
#[cfg(feature = "custody")]
pub struct FileSigner {
    inner: kms::SoftwareSigningRoot,
}

#[cfg(feature = "custody")]
impl FileSigner {
    /// Build a dev signer from raw key material.
    ///
    /// Refuses unless `MIDGARD_ALLOW_FILE_SIGNING_KEY=1`. The check is here, at
    /// construction, so no code path can obtain a `FileSigner` and then discover
    /// it was not permitted.
    pub fn new(
        provider: CryptoProvider,
        key_id: &str,
        verifying_key: VerifyingKey,
        signing_key: SigningKey,
    ) -> Result<Self, SignError> {
        if !file_signing_permitted() {
            return Err(SignError::FileSigningNotPermitted);
        }
        let inner =
            kms::SoftwareSigningRoot::from_key(provider, key_id, verifying_key, signing_key)
                .map_err(|e| SignError::Custodian(e.to_string()))?;
        Ok(Self { inner })
    }
}

/// Identity and algorithm only — never key material. Matches
/// `kms::SoftwareSigningRoot`'s redacting `Debug`.
#[cfg(feature = "custody")]
impl core::fmt::Debug for FileSigner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileSigner")
            .field("key_id", &self.inner.key_id())
            .field("algorithm", &self.inner.algorithm().as_str())
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "custody")]
impl SigningBackend for FileSigner {
    fn key_id(&self) -> &str {
        self.inner.key_id()
    }

    fn sign(&self, msg: &[u8]) -> Result<Signature, SignError> {
        self.inner
            .sign(msg)
            .map_err(|e| SignError::Custodian(e.to_string()))
    }

    fn verifying_key(&self) -> &VerifyingKey {
        self.inner.verifying_key()
    }
}

#[cfg(all(test, feature = "custody"))]
mod tests {
    use super::*;
    use kms::KmsError;

    /// A custodian that refuses every request — an unreachable KMS.
    pub struct DeadRoot {
        vk: VerifyingKey,
    }

    impl SigningRoot for DeadRoot {
        fn key_id(&self) -> &str {
            "dead-key"
        }
        fn algorithm(&self) -> SigId {
            SigId::MlDsa87
        }
        fn verifying_key(&self) -> &VerifyingKey {
            &self.vk
        }
        fn sign(&self, _msg: &[u8]) -> Result<Signature, KmsError> {
            Err(KmsError::Sign("KMS unreachable".into()))
        }
    }

    #[test]
    fn kms_signer_propagates_custodian_failure() {
        let signer = KmsSigner::new(DeadRoot {
            vk: VerifyingKey::new(SigId::MlDsa87, vec![0u8; 8]),
        });
        let err = signer.sign(b"payload").expect_err("must fail closed");
        assert!(matches!(err, SignError::Custodian(_)));
        assert!(err.to_string().contains("KMS unreachable"));
    }

    #[test]
    fn kms_signer_signs_through_a_real_custodian() {
        let mut rng = pqc_agility::OsRng;
        let root = kms::SoftwareSigningRoot::generate(
            crate::provider(),
            "release-key-1",
            kms::RELEASE_SIG_ALG,
            &mut rng,
        )
        .expect("generate");
        let signer = KmsSigner::new(root);
        assert_eq!(signer.key_id(), "release-key-1");
        let sig = signer.sign(b"payload").expect("sign");
        assert_eq!(sig.alg(), SigId::MlDsa87);
    }

    #[test]
    fn no_ambient_key_location_exists_in_this_crate() {
        // A default key location is exactly the ambient-credential failure the
        // gate exists to prevent, so assert its absence in the source.
        // The needles are assembled at runtime so this test does not match
        // itself. [AC-RF-SC-1d]
        let home = "midgard";
        let banned = [
            format!("~/.{home}"),
            format!(".{home}/"),
            format!("/etc/{home}"),
            format!("id_{}", "mldsa"),
            format!("default{}key", '_'),
            format!("SIGNING{}KEY_PATH", '_'),
        ];
        for (name, file) in [
            ("backend.rs", include_str!("backend.rs")),
            ("keystore.rs", include_str!("keystore.rs")),
            ("lib.rs", include_str!("lib.rs")),
        ] {
            for needle in &banned {
                assert!(
                    !file.contains(needle.as_str()),
                    "{name} contains {needle:?}, which looks like an ambient key location"
                );
            }
        }
    }
}
