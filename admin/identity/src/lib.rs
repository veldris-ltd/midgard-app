//! # admin-identity — role-separated admin RBAC + MFA/step-up sessions
//!
//! The identity core of the Admin Control Panel. It is **pure logic with zero
//! dependencies** (the same discipline as `crypto/policy`): a deterministic RBAC
//! model plus an admin-session state machine, side-effect-free and clock-injected
//! so every decision is byte-reproducible and testable.
//!
//! It exists to make three SAD_07 invariants concrete and enforceable:
//!
//! - **No all-powerful role.** Roles are built from *subsets* of the capability
//!   universe; [`RoleCatalog::any_role_all_powerful`] proves none holds all of it,
//!   and the [`ROLE_AUDITOR`] role is strictly read-only. `[FR-AD-2]` `[AC-AD-2]`
//! - **Scoped, expiring bindings.** A [`RoleBinding`] grants a role to an admin
//!   only within a [`ResourceScope`] and a half-open validity window. `[FR-AD-2]`
//! - **MFA + step-up.** An [`AdminSession`] requires multi-factor sign-in, and
//!   entering an admin scope requires a fresh, phishing-resistant step-up
//!   re-authentication. `[AC-AD-1]` `[STD:OWASP ASVS V2]`
//!
//! The privilege / dual-control service (`admin-privilege`) composes this crate:
//! it reads capabilities and roles from the [`IdentityDirectory`] and checks
//! sessions here, then adds risk classification and the dual-control guard.

pub mod binding;
pub mod capability;
pub mod directory;
pub mod durable;
pub mod error;
pub mod passkeys;
pub mod role;
pub mod session;
pub mod webauthn;

pub use binding::{AdminId, ResourceScope, RoleBinding, Subsystem};
pub use capability::Capability;
pub use directory::IdentityDirectory;
pub use durable::DurableIdentityDirectory;
pub use error::IdentityError;
pub use passkeys::DurableAdminPasskeys;
pub use role::{
    AdminRole, RoleCatalog, ROLE_AUDITOR, ROLE_IAM_ADMIN, ROLE_MODEL_STEWARD, ROLE_RELEASE_MANAGER,
    ROLE_SECURITY_OFFICER,
};
pub use session::{AdminAuthenticator, AdminCredential, AdminSession, AuthFactor};
pub use webauthn::{
    verify_admin_assertion, AdminChallenge, RegisteredPasskey, SoftwareAdminAuthenticator,
    WebAuthnAssertion, PASSKEY_SIG_ALG,
};
