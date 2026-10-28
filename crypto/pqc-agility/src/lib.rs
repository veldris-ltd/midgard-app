//! # pqc-agility — the crypto-agility shim
//!
//! One interface for post-quantum key-encapsulation and signatures, with
//! algorithms **negotiated, versioned, and hot-swappable without data migration**
//! (`[NFR-PQ-4]`), hybrid-by-default (`[NFR-PQ-1]`), failing closed when no common
//! post-quantum algorithm exists (`[AC-M-1]`, `[AC-M-8]`).
//!
//! The vetted lattice crypto lives behind a trait; concrete backends are
//! feature-gated:
//! - `test-backend` — a deterministic, **INSECURE** backend for tests only.
//! - `backend-fips` — the production backend (Phase 1): pure-Rust, ACVP-validated
//!   FIPS 203/204/205 + X25519; mutually exclusive with `test-backend`.
//!
//! With no features the traits, algorithm ids, negotiation, registry, hash, and
//! hybrid combiner all still compile — only the concrete algorithms are gated.

#[cfg(feature = "backend-fips")]
pub mod aead;
pub mod algorithm;
pub mod backend;
pub mod error;
pub mod hash;
pub mod hybrid;
pub mod kem;
pub mod negotiate;
pub mod registry;
pub mod rng;
pub mod secret;
pub mod shamir;
pub mod sign;

pub use algorithm::{KemId, KemSuiteId, SigId};
pub use error::{CryptoError, NegotiationError};
pub use hash::{ct_eq, hkdf_sha256_32, hmac_sha256, sha256, Digest, Sha256};
pub use hybrid::combine_secrets;
pub use kem::{Ciphertext, Kem, KemPublicKey, KemSecretKey, KemSizes, SharedSecret};
pub use negotiate::{negotiate_kem, negotiate_sig, Capabilities};
pub use registry::{shared, CryptoProvider, SharedProvider};
pub use rng::Rng;
pub use secret::{SecretArray, SecretBytes};
pub use sign::{Signature, SignatureScheme, SigningKey, VerifyingKey};

#[cfg(any(test, feature = "test-backend"))]
pub use rng::SeedRng;

#[cfg(feature = "backend-fips")]
pub use rng::OsRng;

#[cfg(feature = "backend-fips")]
pub use aead::{Aes256GcmAead, AES256_KEY_LEN, GCM_NONCE_LEN, GCM_TAG_LEN};
