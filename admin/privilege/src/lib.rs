//! # admin-privilege — risk classification + the dual-control guard
//!
//! The authority that decides whether an admin action may proceed. It embodies
//! SAD_07's `KDD-2 — risk drives control`: every admin action class carries a
//! risk level 1..=5, and [`controls_for`] maps that risk to the controls the
//! action must satisfy, so control strength scales with danger automatically.
//! `[FR-AD-5]` `[KDD-2]`
//!
//! For any high-risk action (risk ≥ 4: key rotation, privilege escalation, model
//! revocation, fleet-wide release, break-glass, and panel privilege-logic change)
//! the [`PrivilegeService`] requires a **second, distinct approver** and denies
//! self-approval through a separation-of-duties check. A change to the panel's own
//! privilege logic additionally requires a **security officer** approver. Every
//! request, approval, and rejection — including denied ones — is sealed into a
//! PQ-signed, append-only audit chain (`audit-writer`), the same discipline as the
//! rest of the suite. `[FR-AD-5]` `[AC-AD-3]` `[AC-AD-4]` `[AC-AD-13]`
//!
//! It composes `admin-identity`: capabilities, roles, scoped/expiring bindings,
//! and MFA/step-up sessions are read from there, never re-implemented here.
//!
//! Policy simulation, time-boxed single-use authorisation tokens, and the calls
//! into subsystem admin APIs are added by the action orchestrator in a later
//! phase; this crate owns the *authorisation* decision.

pub mod control;
pub mod durable;
pub mod error;
pub mod risk;
pub mod service;

pub use control::{controls_for, RequiredControls};
pub use durable::{DurablePrivilege, AUDIT_FILE as LEDGER_FILE};
pub use error::PrivError;
pub use risk::{ActionClass, RiskLevel};
pub use service::{ActionEntry, ActionState, AdminActionRequest, Disposition, PrivilegeService};
