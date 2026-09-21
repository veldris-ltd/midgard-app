//! # forge-dataset-store — content-addressed, PQ-sealed dataset versioning
//!
//! The storage foundation of Forge: every dataset — a raw connector ingest or a
//! curated pipeline output — is a **content-addressed, immutable snapshot** sealed
//! at rest with AES-256-GCM under an ML-KEM-1024-wrapped key, carrying full
//! lineage and reachable through named branch refs. It is the shared dataset
//! *contract* the low-code builder and the pro-code IDE both author against.
//! [FR-F-3] [FR-F-1] [NFR-PQ-2]
//!
//! ## Reproducibility (AC-F-2)
//! A dataset's identity is the SHA-256 of its [`codec`]-canonical bytes, taken
//! over the *plaintext*. Sealing uses a fresh nonce each time, but the content id
//! does not depend on it — so a pipeline built twice from the same inputs yields
//! byte-identical versions with equal content ids, across independent stores and
//! hosts, fully offline. [AC-F-2] [NFR-PRV-1]
//!
//! ## At rest (AC-F-1)
//! [`DatasetStore`] holds a [`kms::Kms`] over a caller-supplied [`kms::TrustRoot`]
//! and seals every committed snapshot, binding its content id as AAD. It owns no
//! key material, so a hardware root drops in unchanged. [NFR-PQ-2]
//!
//! ## Layout
//! - [`value`] / [`schema`] / [`dataset`] — the typed dataset contract.
//! - [`codec`] — the canonical, self-describing encoding (content addressing + seal input).
//! - [`version`] / [`lineage`] — content ids and provenance headers.
//! - [`store`] — the sealed, branch-addressed store.

pub mod codec;
pub mod dataset;
pub mod durable;
pub mod error;
pub mod lineage;
pub mod schema;
pub mod store;
pub mod value;
pub mod version;

pub use dataset::{Dataset, Row};
pub use durable::DurableDatasetStore;
pub use error::{DataError, StoreError};
pub use lineage::Lineage;
pub use schema::{Column, ColumnType, Schema};
pub use store::{DatasetStore, StoredVersion};
pub use value::DataValue;
pub use version::{ContentId, DatasetVersion};
