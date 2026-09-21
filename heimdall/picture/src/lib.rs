//! # heimdall-picture — the common-operating-picture model
//!
//! The shared data contract every Heimdall container authors against: the feed
//! gateway mints [`Observation`]s, the fusion engine correlates them into
//! [`TrackedEntity`]s laid out across toggleable [`Layer`]s in a [`Picture`], and
//! picture-sync replicates that picture across nodes as a signed [`PictureDelta`].
//! [FR-H-1] [FR-H-2] [FR-H-6]
//!
//! ## Canonical, offline, deterministic
//! [`codec`] gives every [`Observation`] and [`PictureDelta`] a single
//! byte-encoding: the same value encodes to the same bytes on any host with no
//! clock or I/O, so a feed source can sign an observation and a peer can verify a
//! replicated delta bit-for-bit. That is what lets feed authentication ([FR-H-3])
//! and PQ-mTLS replication ([FR-H-6]) rest on a stable signing preimage.
//! [NFR-PRV-1]
//!
//! ## No crypto here
//! This crate is pure `std`: it defines *what* is signed and replicated, never
//! *how*. The post-quantum signatures live in `heimdall-feed-gateway` and
//! `heimdall-picture-sync`, which sign/verify over these canonical bytes.

pub mod codec;
pub mod delta;
pub mod entity;
pub mod error;
pub mod id;
pub mod layer;
pub mod observation;
pub mod picture;

pub use delta::PictureDelta;
pub use entity::{Classification, ClassificationSource, Track, TrackedEntity};
pub use error::PictureError;
pub use id::{EntityId, FeedId, LayerId, ObservationId};
pub use layer::Layer;
pub use observation::{Attr, Observation};
pub use picture::Picture;
