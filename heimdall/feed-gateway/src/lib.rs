//! # heimdall-feed-gateway — PQ-authenticated feed ingest
//!
//! The warden of the picture boundary. Every observation arrives as a
//! [`SignedObservation`]; the gateway verifies its post-quantum source signature
//! against the feed's registered verifying key and **rejects anything unverified**
//! before it can influence the operating picture. [FR-H-3] [NFR-PQ-3]
//!
//! ## The type-level guarantee (AC-H-2)
//! The only way to obtain a [`VerifiedObservation`] — the sole input the fusion
//! engine accepts — is to pass [`FeedGateway::authenticate`]. An unknown feed, an
//! algorithm mismatch, or a bad signature returns a [`FeedError`] instead, so a
//! spoofed or unsigned feed is *unrepresentable* downstream, not merely filtered.
//!
//! ## Agility, offline
//! Verification resolves the signature scheme from the shared
//! [`pqc_agility::CryptoProvider`] by the signature's algorithm id, so feeds can
//! use different PQ algorithms and algorithms can rotate with no code change. The
//! whole path is local — no outbound dependency (AC-H-7).

pub mod authenticator;
pub mod error;
pub mod registry;
#[cfg(feature = "backend-fips")]
pub mod transport;

pub use authenticator::{sign_observation, FeedGateway, SignedObservation, VerifiedObservation};
pub use error::FeedError;
pub use registry::FeedRegistry;
#[cfg(feature = "backend-fips")]
pub use transport::{
    decode_signed_observation, encode_signed_observation, serve_feed_connection, FeedClient,
    FeedListener, FeedTransportError, FEED_AAD,
};
