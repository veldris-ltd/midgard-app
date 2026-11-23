//! # ontology-import — the ontology file and the import format, once
//!
//! What `mg-admin import` and the HTTP gateway read, parsed and checked here so every
//! reader applies the same rules and gives the same refusals:
//!
//! - [`ontology_file`]: a deployment's object types, link types and actions
//!   (`contracts/schema/ontology.v1.schema.json`);
//! - [`import_file`]: objects and links as JSON Lines, checked against an ontology, all
//!   or nothing (`contracts/schema/import-record.v1.schema.json`);
//! - [`json`], [`strict_json`] and [`marking`]: the std-only JSON codec, the strict
//!   readers and the marking bands they are built on, which the mesh services share
//!   (`service-runtime` re-exports them).
//!
//! Pure: no file, clock or network is read here, so the client core links it too (the
//! Apple apps' "Import JSON files", `docs/guides/IMPORT_YOUR_OWN_DATA.md`).
//! `[DD-1]` `[RC-10]` `[NFR-SEC-2]`

#![forbid(unsafe_code)]

pub mod import_file;
pub mod json;
pub mod marking;
pub mod ontology_file;
pub mod strict_json;
