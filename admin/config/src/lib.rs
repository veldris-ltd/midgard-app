//! # admin-config — versioned, GitOps-style admin configuration
//!
//! All admin configuration is stored with the same discipline as source code: a
//! change is a **commit** with a content hash, a parent link to the prior commit,
//! an author and message, and a post-quantum (ML-DSA) signature over that
//! metadata. History is append-only and independently verifiable; **rollback** is
//! a forward commit that restores an earlier state — never a history rewrite — and
//! any two versions can be **diffed**. `[FR-AD-7]` `[AC-AD-7]`
//!
//! Signing goes through the `pqc-agility` shim (the caller supplies the backend),
//! so commit signatures rotate algorithms with the rest of the platform and the
//! store works fully offline. `[NFR-PQ-3]` `[NFR-PRV-1]`

pub mod commit;
pub mod diff;
pub mod durable;
pub mod error;
pub mod reconcile;
pub mod state;
pub mod store;

pub use commit::{commit_preimage, Commit};
pub use diff::ConfigDiff;
pub use durable::DurableConfigStore;
pub use error::ConfigError;
pub use reconcile::{
    DriftReport, DriftStatus, ReconcileError, ReconcileOutcome, ReconcileResult, Reconciler,
    Subsystem, SubsystemDrift, SubsystemOutcome,
};
pub use state::ConfigState;
pub use store::ConfigStore;
