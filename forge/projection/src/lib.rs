//! # forge-projection — curated datasets → the object fabric
//!
//! The projection mapper turns a curated [`forge_dataset_store::Dataset`] into
//! object and link instances in the Midgard object fabric, **propagating the
//! dataset's marking onto every projected object** so classification is never lost
//! at the data→object boundary. [FR-F-4] [NFR-SEC-2]
//!
//! - [`ObjectMapping`] / [`project_objects`] — rows → typed object instances, each
//!   stamped with the mapping's [`policy::Marking`] and allowed purposes.
//! - [`LinkMapping`] / [`project_links`] — rows → directed links between object ids.
//!
//! Projection fails closed: an unmapped column, an unusable primary key, or a value
//! type with no fabric representation aborts the whole projection.

pub mod error;
pub mod mapping;
pub mod project;

pub use error::ProjectionError;
pub use mapping::{LinkMapping, ObjectMapping, PropertyMapping};
pub use project::{project_links, project_objects, ProjectionReport};
