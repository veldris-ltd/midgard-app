//! # heimdall-fusion — streaming fusion into the common picture
//!
//! The engine that turns a stream of *verified* observations into one live
//! operating picture. It correlates observations by their key into
//! [`heimdall_picture::TrackedEntity`]s, maintains each entity's temporal track,
//! lays entities across toggleable layers, classifies them via an injected
//! [`heimdall_classify::Classifier`], and projects them into the Midgard object
//! fabric with markings. [FR-H-1] [FR-H-2]
//!
//! ## Only verified feeds fuse (AC-H-2)
//! [`FusionEngine::ingest`] accepts a
//! [`heimdall_feed_gateway::VerifiedObservation`] — the type minted only by the
//! feed gateway after a successful PQ signature check — so an unauthenticated feed
//! cannot reach fusion by construction.
//!
//! ## Standalone + offline (AC-H-6, AC-H-7)
//! Correlation, tracking, layering, and the rules classifier are pure and local:
//! no model, no network, no clock. With Mimir absent and with no outbound
//! connectivity the engine still produces a fully fused, rules-classified picture.

pub mod engine;
pub mod fabric_sync;
pub mod layering;

pub use engine::FusionEngine;
pub use fabric_sync::{project_entity, sync_to_fabric, ENTITY_TYPE_ID};
pub use layering::LayerPolicy;
