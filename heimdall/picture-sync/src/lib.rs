//! # heimdall-picture-sync — PQ-authenticated multi-node replication
//!
//! Replicates the common operating picture across nodes and sites. A node exports
//! its picture as a [`heimdall_picture::PictureDelta`], signs the delta's canonical
//! bytes with its post-quantum key, and transmits it; a peer **verifies the source
//! signature before merging**, rejecting any unauthenticated or tampered delta.
//! Merge is last-write-wins by `last_seen`, so nodes exchanging deltas in any order
//! converge. [FR-H-6] [NFR-PQ-3] [AC-H-5]
//!
//! ## Relationship to PQ-mTLS
//! The signature over the delta bytes is the *source-authenticity* property Heimdall
//! needs; in deployment those bytes ride a `pq-mtls` [`Session`] (the same PQ,
//! mutually-authenticated, hybrid-KEM channel the rest of the suite uses), which
//! adds confidentiality and replay protection at the record layer. The
//! [`DeltaLink`] seam here abstracts that transport — a [`LoopbackLink`] backs
//! in-process tests, a `pq-mtls` session backs the wire — exactly as `pq-mtls`
//! itself models its own byte transport as a seam.
//!
//! ## Offline (AC-H-7)
//! Signing, verification, encode, decode, and merge are all local and
//! deterministic: replication between co-located nodes needs no outbound network.
//!
//! [`Session`]: https://docs.rs/pq-mtls

pub mod error;
pub mod link;
pub mod replicator;
#[cfg(feature = "backend-fips")]
pub mod session_link;

pub use error::SyncError;
pub use link::{loopback_link, DeltaLink, LoopbackLink};
pub use replicator::{Replicator, SignedDelta};
#[cfg(feature = "backend-fips")]
pub use session_link::{
    decode_signed_delta, encode_signed_delta, replicate_round, PeerRegistry, SessionLink, DELTA_AAD,
};
