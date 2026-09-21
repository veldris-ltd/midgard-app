//! # fabric — the Object Fabric service (OFS)
//!
//! Midgard's semantic + kinetic core: the direct answer to Palantir's Ontology.
//! It pairs **data** (object types, properties, links — the semantics) with
//! **decisions** (actions, functions, dynamic security — the kinetics), so what
//! the platform models is not just data but governed change. [DD-1]
//!
//! Layers:
//! - [`types`] — the type registry (OMS): validated object/link/action types and
//!   interfaces (semantic schema).
//! - [`object`] / [`store`] — instances over a horizontally-scalable
//!   [`store::ObjectStore`] with incremental type + inverted-property indexing.
//! - [`query`] — typed queries and search-around, authorised per object at query
//!   time (dynamic security).
//! - [`action`] — the function runtime: pure, inspectable effects.
//! - [`fabric`] — the [`fabric::Fabric`] facade: the typed, policy-enforced,
//!   PQ-audited query/action surface products depend on (the SDK contract, mirror
//!   of the `ObjectFabric` proto). [DD-6]

pub mod action;
pub mod durable;
pub mod error;
pub mod fabric;
pub mod inference;
pub mod object;
pub mod query;
pub mod store;
pub mod types;
pub mod value;

pub use action::{ActionFunction, Effect, FunctionRuntime};
// The ledger seam the fabric is generic over, so consumers name it through one
// surface without depending on `audit-writer` directly. [AC-M-4]
pub use audit_writer::{InMemoryLedger, LedgerStore};
pub use durable::DurableObjectStore;
pub use error::FabricError;
pub use fabric::{ApplyActionResponse, Fabric, QueryResponse};
// The governance spine owns the policy context and audit signature algorithm;
// re-export under the fabric's historical names so consumers use one surface.
pub use governance::{GovContext as PolicyContext, Governance, GOV_SIG_ALG as FABRIC_SIG_ALG};
pub use inference::Inference;
pub use object::{LinkInstance, ObjectInstance};
pub use query::Query;
pub use store::{InMemoryStore, ObjectStore};
pub use types::{
    ActionType, Cardinality, Interface, LinkType, ObjectType, PropertyType, TypeRegistry,
};
pub use value::{Value, ValueType};
