//! # admin-audit — the admin audit ledger
//!
//! One PQ-signed, append-only, tamper-evident chain that records **every** admin
//! action — committed, rolled back, denied, and even simulated — with the full
//! who / what / why / when / approver detail AC-AD-6 requires. It wraps
//! `crypto/audit-writer` (the same hash-chained, ML-DSA-signed ledger the rest of
//! the suite uses) and adds the admin-specific event shape on top, so the panel
//! gets a single exportable governance record with no new crypto. `[NFR-PQ-3]`
//! `[AC-AD-6]`
//!
//! `who` / `what` / `when` are the native `actor` / `action` / `ts` audit fields;
//! `why` (justification), `approver`, and the `outcome` are packed into the signed
//! `resource` field by an injective canonical encoding ([`decode_detail`] reverses
//! it), so they are covered by the entry signature and cannot be altered without
//! detection.
//!
//! The service crates (`admin-sim`, `admin-config`, `admin-breakglass`) are
//! audit-agnostic: they return domain results, and the caller (the action
//! orchestrator, or a test) records an [`AdminEvent`] here. That keeps audit a
//! single cross-cutting concern rather than re-implemented per service.

pub mod error;
pub mod event;
pub mod log;

pub use error::AdminAuditError;
pub use event::{decode_detail, AdminDetail, AdminEvent, AdminOutcome};
pub use log::AdminAuditLog;
