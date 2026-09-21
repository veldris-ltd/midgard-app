//! # bifrost-bundle — signed offline release bundles
//!
//! The air-gap transit path. A bundle packages one or more signed artefacts under
//! a single ML-DSA-signed [`BundleManifest`] that commits to each artefact's
//! identity and digests. An air-gapped site [`import`]s it **fully offline**,
//! verifying the bundle signature, the manifest-to-contents match, and every
//! contained artefact before anything is deployed — and rejects the bundle on any
//! signature failure. [FR-B-5] [AC-B-3]
//!
//! Because Mimir model bundles and client artefacts travel through this same
//! workflow (as `model-bundle` / `client` component artefacts), this closes the
//! AC-MS-3 and AC-C-7 forward links at the Bifrost boundary.
//!
//! [`pack_streaming`] and [`import_streaming`] (the [`stream`] module) do the same for
//! artefacts too large to hold in memory, such as a model's weights: format version 2,
//! written and verified a chunk at a time.

pub mod error;
pub mod manifest;

#[cfg(feature = "backend-fips")]
pub mod bundle;
pub mod file;
#[cfg(feature = "backend-fips")]
pub mod stream;

pub use error::BundleError;
pub use manifest::{BundleEntry, BundleManifest};

#[cfg(feature = "backend-fips")]
pub use bundle::{build_bundle, import, OfflineBundle};
pub use file::{
    from_bytes, read_bundle, to_bytes, write_bundle, MAX_CHUNK_SIZE, STREAMING_FORMAT_VERSION,
};
#[cfg(feature = "backend-fips")]
pub use stream::{import_streaming, pack_streaming, FileArtefact, DEFAULT_CHUNK_SIZE};
