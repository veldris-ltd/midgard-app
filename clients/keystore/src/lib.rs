//! # midgard-client-keystore — device key custody, one API, per-platform floors
//!
//! Key custody differs sharply across targets — Secure Enclave, StrongBox, TPM,
//! Secret Service, WebCrypto — so the core hides them behind one trait and a
//! **minimum-security floor** each platform must meet. `[FR-C-2]`
//!
//! A **key-holding** keystore is, precisely, a [`kms::TrustRoot`]: it owns a
//! non-exportable ML-KEM-1024 keypair and *unwraps inside itself*, so the wrapped
//! data key never leaves custody. That is why `midgard-client-store` reuses the
//! suite-wide KEM-DEM envelope unchanged — the client re-rolls no crypto. `[KDD-1]`
//! `[NFR-PQ-2]`
//!
//! The **browser extension** is the exception (KDD-2): it holds *no* key material
//! and cannot seal locally. [`BrokeredKeystore`] models that — it reports
//! [`SecurityFloor::Brokered`] and `holds_key() == false`, and it deliberately does
//! **not** implement [`kms::TrustRoot`], so the type system forbids using it as a
//! local sealing root. Its privileged operations are brokered (Phase 8.d).
//!
//! ## Non-exportability is structural
//! Neither [`Keystore`] nor [`kms::TrustRoot`] exposes the secret key or the
//! derived data key. [`SoftwareKeystore`] — the deterministic dev/test custody —
//! holds its secret in memory but offers no accessor for it, exactly as a hardware
//! keystore offers none: the only operation is *unwrap*. `[AC-C-2]`

#![forbid(unsafe_code)]

use kms::{SoftwareTrustRoot, TrustRoot};
use pqc_agility::{Ciphertext, CryptoProvider, Kem, KemPublicKey, Rng, SharedSecret};

pub mod auth;
mod error;
pub use auth::{
    verify_assertion, AuthAssertion, AuthChallenge, AuthMechanism, PlatformAuthenticator,
    RegisteredCredential, SoftwareAuthenticator,
};
pub use error::KeystoreError;

/// The custody strength a keystore guarantees, ordered weakest-to-strongest for
/// the key-holding floors. [`Brokered`](SecurityFloor::Brokered) is a distinct
/// case: it holds no key at all, so it satisfies no *local* floor — a brokered
/// keystore cannot back a local store, only a broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecurityFloor {
    /// Holds no key material; every privileged op is brokered. (Extension.)
    Brokered,
    /// A software/WebCrypto non-extractable key: not exportable, but not
    /// hardware-sealed. The minimum a *key-holding* client may run at.
    NonExtractable,
    /// An OS-encrypted keyring (Linux Secret Service) without a guaranteed TPM.
    EncryptedKeyring,
    /// Sealed to a TPM (Windows DPAPI+TPM) where present.
    TpmSealed,
    /// Hardware-backed and non-exportable (Apple Secure Enclave, Android
    /// StrongBox). The strongest floor.
    HardwareBacked,
}

/// The delivery targets, each mapped to the custody floor SAD_06 §11.2 requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformProfile {
    /// iOS / macOS — Keychain + Secure Enclave.
    Apple,
    /// Android — Keystore + StrongBox where present.
    Android,
    /// Windows — DPAPI / TPM.
    Windows,
    /// Linux — Secret Service / TPM.
    Linux,
    /// Browser web app — WebCrypto non-extractable keys.
    WebApp,
    /// Browser extension — no raw key custody; brokered.
    Extension,
}

impl PlatformProfile {
    /// The custody floor this platform must meet. `[FR-C-2]`
    pub fn floor(self) -> SecurityFloor {
        match self {
            PlatformProfile::Apple | PlatformProfile::Android => SecurityFloor::HardwareBacked,
            PlatformProfile::Windows => SecurityFloor::TpmSealed,
            PlatformProfile::Linux => SecurityFloor::EncryptedKeyring,
            PlatformProfile::WebApp => SecurityFloor::NonExtractable,
            PlatformProfile::Extension => SecurityFloor::Brokered,
        }
    }
}

/// Uniform custody metadata over every platform key store. A key-holding keystore
/// additionally implements [`kms::TrustRoot`]; a brokered one does not.
pub trait Keystore {
    /// The custody floor this store guarantees.
    fn floor(&self) -> SecurityFloor;
    /// Whether this store holds a local key (false ⇒ brokered).
    fn holds_key(&self) -> bool;
    /// A stable identifier for the custody root (or broker).
    fn key_id(&self) -> &str;
}

/// Deterministic in-process custody for dev and tests. Holds a non-exportable
/// ML-KEM-1024 root and declares a floor (default [`SecurityFloor::HardwareBacked`]
/// to model a Secure-Enclave-class device; override to test floor enforcement).
///
/// The secret key is owned by the wrapped [`SoftwareTrustRoot`] and never exposed:
/// the only operation is [`TrustRoot::unwrap`], performed internally. A real
/// platform keystore drops in as another [`Keystore`] + [`TrustRoot`] with the
/// secret sealed in hardware.
pub struct SoftwareKeystore {
    root: SoftwareTrustRoot,
    floor: SecurityFloor,
}

impl SoftwareKeystore {
    /// Generate a fresh non-exportable root at the given floor.
    pub fn generate(
        provider: &CryptoProvider,
        key_id: &str,
        floor: SecurityFloor,
        rng: &mut dyn Rng,
    ) -> Result<Self, KeystoreError> {
        if floor == SecurityFloor::Brokered {
            // A brokered custody holds no key — use BrokeredKeystore instead.
            return Err(KeystoreError::BrokeredCannotHoldKey);
        }
        let root = SoftwareTrustRoot::generate(provider, key_id, rng)
            .map_err(|e| KeystoreError::RootGeneration(e.to_string()))?;
        Ok(Self { root, floor })
    }
}

impl Keystore for SoftwareKeystore {
    fn floor(&self) -> SecurityFloor {
        self.floor
    }
    fn holds_key(&self) -> bool {
        true
    }
    fn key_id(&self) -> &str {
        self.root.key_id()
    }
}

// A key-holding keystore *is* a KMS trust root: it unwraps data keys inside its
// own custody, so the wrapped key never leaves the device. This is the seam that
// lets the local store reuse the suite-wide envelope with zero new crypto.
impl TrustRoot for SoftwareKeystore {
    fn key_id(&self) -> &str {
        self.root.key_id()
    }
    fn public_key(&self) -> &KemPublicKey {
        self.root.public_key()
    }
    fn unwrap(&self, kem: &dyn Kem, wrapped: &Ciphertext) -> Result<SharedSecret, kms::KmsError> {
        self.root.unwrap(kem, wrapped)
    }
}

/// The extension's custody: none. It brokers every privileged operation and holds
/// no key, so it is not a [`kms::TrustRoot`] and cannot seal locally. `[FR-C-8]`
/// `[KDD-2]`
pub struct BrokeredKeystore {
    broker_id: String,
}

impl BrokeredKeystore {
    /// A brokered custody naming the broker it delegates to (enclosure or
    /// companion app).
    pub fn new(broker_id: impl Into<String>) -> Self {
        Self {
            broker_id: broker_id.into(),
        }
    }
}

impl Keystore for BrokeredKeystore {
    fn floor(&self) -> SecurityFloor {
        SecurityFloor::Brokered
    }
    fn holds_key(&self) -> bool {
        false
    }
    fn key_id(&self) -> &str {
        &self.broker_id
    }
}

#[cfg(all(test, feature = "backend-fips"))]
mod tests {
    use super::*;
    use pqc_agility::backend::fips::provider as fips_provider;

    struct SeedRng(u64);
    impl Rng for SeedRng {
        fn fill(&mut self, dest: &mut [u8]) {
            for b in dest {
                self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
                *b = (self.0 >> 33) as u8;
            }
        }
    }

    #[test]
    fn software_keystore_holds_a_non_exportable_key() {
        let provider = fips_provider();
        let ks = SoftwareKeystore::generate(
            &provider,
            "device-root",
            SecurityFloor::HardwareBacked,
            &mut SeedRng(1),
        )
        .unwrap();
        assert!(ks.holds_key());
        assert_eq!(ks.floor(), SecurityFloor::HardwareBacked);
        assert_eq!(Keystore::key_id(&ks), "device-root");
        // It is usable as a KMS trust root — the only key operation it exposes.
        assert_eq!(ks.public_key().suite(), kms::WRAP_SUITE);
    }

    #[test]
    fn brokered_keystore_holds_no_key() {
        let bk = BrokeredKeystore::new("enclosure");
        assert!(!bk.holds_key());
        assert_eq!(bk.floor(), SecurityFloor::Brokered);
    }

    #[test]
    fn platform_floors_match_sad_table() {
        assert_eq!(
            PlatformProfile::Apple.floor(),
            SecurityFloor::HardwareBacked
        );
        assert_eq!(
            PlatformProfile::Android.floor(),
            SecurityFloor::HardwareBacked
        );
        assert_eq!(PlatformProfile::Windows.floor(), SecurityFloor::TpmSealed);
        assert_eq!(
            PlatformProfile::Linux.floor(),
            SecurityFloor::EncryptedKeyring
        );
        assert_eq!(
            PlatformProfile::WebApp.floor(),
            SecurityFloor::NonExtractable
        );
        assert_eq!(PlatformProfile::Extension.floor(), SecurityFloor::Brokered);
    }

    #[test]
    fn floor_ordering_is_weakest_to_strongest() {
        assert!(SecurityFloor::NonExtractable < SecurityFloor::HardwareBacked);
        assert!(SecurityFloor::TpmSealed < SecurityFloor::HardwareBacked);
        assert!(SecurityFloor::EncryptedKeyring < SecurityFloor::TpmSealed);
    }

    #[test]
    fn generating_a_brokered_root_is_rejected() {
        let provider = fips_provider();
        let result =
            SoftwareKeystore::generate(&provider, "x", SecurityFloor::Brokered, &mut SeedRng(2));
        assert!(matches!(result, Err(KeystoreError::BrokeredCannotHoldKey)));
    }
}
