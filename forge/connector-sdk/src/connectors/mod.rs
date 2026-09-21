//! Reference connectors. Each is feature-gated so a deployment compiles in only
//! the source families it trusts (a smaller sandboxed surface). The file and in-memory
//! connectors are offline; the network connectors reach only the `host:port` the
//! sandbox permits. [FR-F-2] [NFR-PRV-1]

#[cfg(feature = "connector-file")]
pub mod file;

#[cfg(feature = "connector-inmem")]
pub mod inmem;

#[cfg(feature = "connector-api")]
pub mod api;

#[cfg(feature = "connector-db")]
pub mod postgres;

#[cfg(feature = "connector-objstore")]
pub mod s3;

#[cfg(feature = "connector-stream")]
pub mod kafka;
