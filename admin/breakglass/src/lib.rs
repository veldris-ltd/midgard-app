//! # admin-breakglass — time-boxed emergency elevation
//!
//! Emergency access is the panel's most dangerous capability, so it is the most
//! heavily controlled. A break-glass grant is `[FR-AD-8]` `[AC-AD-8]`:
//!
//! - **time-boxed** — it carries an explicit expiry and stops being active the
//!   moment it passes;
//! - **justified** — a non-empty justification is mandatory;
//! - **two-person** — the requester and the approver each come as an
//!   [`AdminSession`](admin_identity::AdminSession), which exists only after that
//!   administrator's own passkey assertion verified. The approver must be a
//!   *different* authenticated administrator with a fresh re-authentication, and both
//!   identities are recorded in the grant and the audit ledger. A typed approver name
//!   is no longer accepted anywhere. Authenticated is not enough: in the
//!   [`IdentityDirectory`](admin_identity::IdentityDirectory) the requester must hold
//!   `breakglass.request` and the approver `breakglass.approve` (the security
//!   officer's) through role bindings active now and covering the grant's scope.
//!   `[MG-SEC-07]`
//! - **real-time alerted** — granting raises an alert immediately, and an expiry
//!   that has not yet been reviewed raises another;
//! - **forced post-hoc review** — an expired grant remains outstanding until a
//!   reviewer signs it off; [`BreakGlassService::pending_reviews`] surfaces the
//!   backlog so review cannot be quietly skipped.
//!
//! It is a pure state machine over an injected clock and an [`Alerter`] sink, with
//! no network or storage of its own, so it functions fully in-enclosure /
//! air-gapped. `[AC-AD-12]` The orchestrator records each transition to the admin
//! audit ledger. `[AC-AD-6]`

pub mod alert;
pub mod controller;
pub mod durable;
pub mod error;
pub mod grant;
pub mod service;
pub mod token;

pub use alert::{
    AlertBus, AlertBusError, AlertKind, Alerter, BreakGlassAlert, DurableAlerter, RecordingAlerter,
    SecurityOfficerBus,
};
pub use controller::BreakGlassController;
pub use durable::DurableBreakGlass;
pub use error::BreakGlassError;
pub use grant::{BreakGlassGrant, BreakGlassState, GrantRequest, Review};
pub use service::{BreakGlassService, CAP_APPROVE, CAP_REQUEST};
pub use token::{BreakGlassAuthority, CapabilityToken};
