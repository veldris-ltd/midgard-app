//! # heimdall-store — durable Heimdall stores
//!
//! Persistence *behind* the pure-std `heimdall-picture` and `heimdall-workflow`
//! types, so the common operating picture and in-flight human-gated workflows
//! survive a restart:
//! - [`DurablePicture`] — the fused picture, KMS-sealed at rest (AES-256-GCM under
//!   an ML-KEM-1024-wrapped key), snapshotted atomically on each change and
//!   recovered on boot.
//! - [`DurableWorkflow`] — the workflow engine over a durable, PQ-signed audit
//!   [`audit_writer::FileLedger`], with the in-flight action state snapshotted
//!   KMS-sealed alongside it, so both the decision lineage and the pending
//!   workflows are recovered on boot.
//!
//! Everything at rest is ciphertext; a tampered blob fails closed. [FR-H-1]
//! [FR-H-5] [RC-13]

pub mod error;
pub mod picture_store;
pub(crate) mod util;
pub mod workflow_store;

pub use error::StoreError;
pub use picture_store::DurablePicture;
pub use workflow_store::DurableWorkflow;
