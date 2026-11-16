//! # midgard-client-ffi — the UniFFI surface over the client core
//!
//! Swift (and later Kotlin) calls this crate; this crate calls `midgard-client-core`.
//! Nothing here holds key material, merges sync state or decides clearance: those
//! stay in the core (KDD-1). The only foreign code the core calls back is
//! [`KeyRoot`], the platform key root that seals and unseals the core's 32-byte data
//! key. Every byte string that crosses is sealed (the root, the device-audit key, the
//! state file) or public (rows the persona may see): no key material is ever
//! returned to the shell. `[FR-C-1]` `[DD-6]` `[AC-C-13]`
//!
//! The bundled dataset is opened only with `DemoEnclosure::open_pinned`, under the
//! anchors compiled into the core; this crate never enables the core's
//! `demo-unpinned` feature (`tools/ci/no-test-backend.sh`). `[NFR-PQ-3]`

// No `#![forbid(unsafe_code)]` here: UniFFI's scaffolding and derives emit `unsafe`
// FFI glue, and `forbid` cannot be lifted for generated code. `deny` stops any
// hand-written `unsafe`; `grep -rnE 'unsafe +(fn|impl|\{)' clients/ffi/src` must stay
// empty.
#![deny(unsafe_code)]

uniffi::setup_scaffolding!();

mod client;
mod error;
pub mod generated;
mod key_root;
pub mod motion;
mod rows;

pub use client::*;
pub use error::ClientError;
pub use generated::KeyValue;
pub use key_root::{KeyRoot, KeyRootError};
pub use rows::{row_types, ConsoleRow, RowItem, SignatureStatus};

/// The core version every shell links. `[AC-C-1]`
#[uniffi::export]
pub fn core_version() -> String {
    midgard_client_core::CORE_VERSION.to_string()
}
