//! # bifrost-artefact — signed, SBOM-attested deployable artefacts
//!
//! The supply-chain trust root of Bifrost. Every deployable is an
//! [`ArtefactManifest`] binding the artefact's identity to the SHA-256 of its
//! payload **and** of its CycloneDX SBOM, signed with ML-DSA (FIPS 204). Because
//! both digests are inside the signed bytes, tampering with the deployable or its
//! SBOM breaks the signature. [FR-B-1] [NFR-PQ-3] [NFR-OSS-1]
//!
//! The [`ArtefactRegistry`] verifies **on ingest** and **before every deploy**,
//! failing closed on an unsigned or tampered artefact — the KDD that "deploys fail
//! closed on trust". [FR-B-4] [AC-B-1] [AC-B-4]
//!
//! - [`manifest`] — the signed header + [`ArtefactRef`].
//! - [`sbom`] — deterministic CycloneDX 1.5 generation.
//! - [`signed`] — build-and-sign + fail-closed verify.
//! - [`registry`] — the verify-on-ingest / verify-before-deploy store.

pub mod error;
pub mod manifest;
pub mod sbom;
pub mod signed;

#[cfg(feature = "backend-fips")]
pub mod durable;
#[cfg(feature = "backend-fips")]
pub mod registry;

pub use error::{RegistryError, VerifyError};
pub use manifest::{ArtefactManifest, ArtefactRef, BIFROST_SIG_ALG};
pub use sbom::{generate_sbom, SbomComponent};
pub use signed::SignedArtefact;

#[cfg(feature = "backend-fips")]
pub use durable::DurableArtefactRegistry;
#[cfg(feature = "backend-fips")]
pub use registry::ArtefactRegistry;
#[cfg(feature = "backend-fips")]
pub use signed::{build_and_sign, verify};
