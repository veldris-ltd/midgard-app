//! # iam — Identity & Access
//!
//! A self-contained, **fully offline** IdP for the Midgard enclosure. It is both
//! the mesh certificate authority (certifying workload identities for PQ-mTLS
//! mutual auth) and the issuer of **PQ-signed access tokens** (ML-DSA / FIPS 204).
//! No database and no outbound network: identities live in an in-memory store and
//! tokens are signed locally, so the IdP runs air-gapped. [NFR-SEC-2] [NFR-PRV-1]
//!
//! Verification is decoupled from issuance — any consumer (e.g. the gateway) can
//! verify a token or certificate with only the issuer's verifying key
//! ([`Idp::verifying_key`] / [`Idp::trust_anchor`]) and a crypto provider.

pub mod auth;
pub mod directory;
pub mod enc;
pub mod federation;
pub mod idp;
pub mod mesh_ca;
pub mod mfa;
pub mod oidc;
pub mod password;
pub mod store;
pub mod token;

pub use auth::{AuthError, Authenticator, MfaProof};
pub use directory::{Credentials, Directory, DurableDirectory, Enrollment, InMemoryDirectory};
pub use federation::{
    federated_login, FederatedClaims, FederatedIdp, FederationError, PqFederation,
};
pub use idp::{Idp, IssueError, IDP_SIG_ALG};
pub use mesh_ca::{CaError, CertStatus, IssuedCert, MeshCa};
pub use mfa::{AuthAssertion, TotpSecret, WebAuthnCredential, WebAuthnError, PASSKEY_SIG_ALG};
pub use oidc::jwks_json;
pub use password::{PasswordHasher, Pbkdf2Hasher, DEFAULT_ITERATIONS};
pub use store::{IdentityStore, UserRecord};
pub use token::{sign_token, verify_token, TokenClaims, TokenError};
