//! # forge-connector-sdk — typed, sandboxed source connectors
//!
//! The ingest edge of Forge. A [`Connector`] is a typed source binding that pulls
//! external data into a validated [`forge_dataset_store::Dataset`]; the SDK then
//! seals-and-commits it as a raw, content-addressed, PQ-encrypted version via
//! [`sync_to_store`]. [FR-F-2] [NFR-PQ-2] [AC-F-1]
//!
//! ## Trust posture
//! - **Sandboxed.** Connectors run under a [`SandboxPolicy`] that denies all
//!   outbound access by default (air-gapped); egress must be explicitly permitted.
//!   [NFR-PRV-1]
//! - **Credentials by reference.** A connector holds a [`CredentialRef`], never a
//!   secret. Secrets are resolved at sync time from the KMS
//!   ([`KmsCredentialResolver`]) and exist only transiently. [NFR-PQ-2]
//! - **Typed at the edge.** Parsing produces a schema-validated dataset, so a
//!   malformed source fails closed at ingest.
//!
//! - **Incremental.** A connector may resume from the checkpoint its previous sync
//!   returned ([`Connector::sync_incremental`], [`SyncOutcome`]).
//!
//! ## Reference connectors (feature-gated)
//! [`connectors::file`] (RFC 4180 delimited text), [`connectors::inmem`] (fixtures /
//! bounded streams), and the network families: `connectors::postgres` (database),
//! `connectors::s3` (S3-compatible object stores) and `connectors::kafka` (streams).
//! Network connectors reach only the `host:port` the sandbox permits, through
//! [`net::connect`]. [FR-F-2] [FR-F-5]

pub mod connector;
pub mod connectors;
pub mod credential;
pub mod csv;
pub mod error;
pub mod http;
pub mod net;
pub mod sigv4;
pub mod sync;
pub mod wire;

pub use connector::{
    Connector, ConnectorDescriptor, ConnectorKind, SandboxPolicy, SyncContext, SyncOutcome,
};
pub use credential::{
    CredentialRef, CredentialResolver, KmsCredentialResolver, NoCredentials, Secret,
};
pub use error::ConnectorError;
pub use sync::sync_to_store;

#[cfg(feature = "connector-api")]
pub use connectors::api::{ApiConnector, Fetcher};
#[cfg(feature = "connector-file")]
pub use connectors::file::{DelimitedConnector, FileConnector};
#[cfg(feature = "connector-stream")]
pub use connectors::kafka::{KafkaConnector, StartFrom};
#[cfg(feature = "connector-db")]
pub use connectors::postgres::PostgresConnector;
#[cfg(feature = "connector-objstore")]
pub use connectors::s3::S3Connector;
