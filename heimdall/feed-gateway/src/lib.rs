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
//!
//! ## Replay protection (VLD-ANB-MIDGARD-0041)
//! A valid signature proves the source, not that the observation is new. Each
//! observation carries its feed's monotonic `sequence` inside the signed bytes
//! (observation format version 2; version 1 is refused at decode). After the
//! signature verifies, and only then, the gateway's [`ReplayGuard`] refuses a
//! sequence of 0, a sequence not above the feed's last admitted one, an
//! `observed_at` outside the [`FreshnessPolicy`] window around the gateway's clock,
//! and an id already in the feed's window of the last [`MAX_SEEN_PER_FEED`] ids.
//! One guard is shared by every connection or request
//! ([`FeedGateway::with_replay_guard`]), and [`FeedGateway::authenticate_from`]
//! binds the observation's feed to the authenticated PQ-mTLS peer, or to a peer the
//! [`FeedRegistry`] delegates it to, so one certified source cannot speak for
//! another. Sources number their observations with a [`FeedSequencer`].

pub mod authenticator;
pub mod error;
pub mod registry;
pub mod replay;
pub mod sequencer;
#[cfg(feature = "backend-fips")]
pub mod transport;

pub use authenticator::{sign_observation, FeedGateway, SignedObservation, VerifiedObservation};
pub use error::FeedError;
pub use registry::FeedRegistry;
pub use replay::{FreshnessPolicy, ReplayGuard, MAX_SEEN_PER_FEED};
pub use sequencer::FeedSequencer;
#[cfg(feature = "backend-fips")]
pub use transport::{
    decode_signed_observation, encode_signed_observation, serve_feed_connection, FeedClient,
    FeedListener, FeedTransportError, FEED_AAD,
};
