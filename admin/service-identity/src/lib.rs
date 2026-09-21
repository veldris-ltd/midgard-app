//! # admin-service-identity — distinct least-privilege identity per subsystem
//!
//! The panel administers many subsystems, and it reaches each one with a **distinct
//! workload identity that carries only that subsystem's narrow service role** — it
//! never holds a superuser credential accepted everywhere. So a compromise of the
//! panel's binding to one subsystem cannot escalate into another. `[FR-AD-1]`
//! `[NFR-SEC-1]` `[AC-AD-9]`
//!
//! This composes the existing crypto identity stack — it does not reinvent it. The
//! `iam` IdP (the mesh CA) certifies each identity; `pq-mtls` provides the
//! [`pq_mtls::WorkloadIdentity`] / [`pq_mtls::IdentityCert`] and the `verify_cert`
//! check. The panel-specific part is the *policy*: exactly one narrow service role
//! per subsystem, and no all-powerful identity.

pub mod identities;
pub mod subsystem;

pub use identities::{AdminServiceIdentities, IdentityError, SubsystemIdentity};
pub use subsystem::{service_role, SUBSYSTEMS};
