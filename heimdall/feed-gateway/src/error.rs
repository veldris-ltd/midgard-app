//! Feed-authentication errors. Every variant is a rejection: there is no code path
//! that admits an observation without a verified signature. [FR-H-3]

use pqc_agility::error::CryptoError;
use pqc_agility::SigId;

use heimdall_picture::{FeedId, ObservationId};
use pq_mtls::ServiceId;

/// Why an observation was rejected from the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    /// The observation's feed is not registered with a verifying key.
    UnknownFeed(FeedId),
    /// The signature's algorithm does not match the feed's registered key.
    AlgorithmMismatch {
        /// The algorithm of the registered feed key.
        expected: SigId,
        /// The algorithm the signature claimed.
        got: SigId,
    },
    /// The signature did not verify against the feed's key and the observation
    /// bytes (spoofed, tampered, or wrong key). [NFR-PQ-3]
    SignatureInvalid(FeedId),
    /// The provider could not supply a backend for the signature algorithm
    /// (unregistered or disabled) — fails closed. Carries the provider's message
    /// (`CryptoError` is not itself comparable).
    Crypto(String),
    /// The observation carries no sequence number (0). [VLD-ANB-MIDGARD-0041]
    SequenceUnset(FeedId),
    /// The observation's sequence is not above the last one admitted from its feed: a
    /// replay, or a feed that restarted its sequence. [VLD-ANB-MIDGARD-0041]
    SequenceNotMonotonic {
        /// The feed.
        feed: FeedId,
        /// The sequence the observation carries.
        got: u64,
        /// The last sequence admitted from the feed.
        last: u64,
    },
    /// The observation's `observed_at` is outside the freshness window around the
    /// gateway's clock. [VLD-ANB-MIDGARD-0041] [VLD-ANB-MIDGARD-0015]
    Stale {
        /// The feed.
        feed: FeedId,
        /// The time the feed gave the observation.
        observed_at: u64,
        /// The gateway's clock when it was checked.
        now: u64,
    },
    /// The observation's id is already in the feed's window of recent ids.
    /// [VLD-ANB-MIDGARD-0041]
    Replayed {
        /// The feed.
        feed: FeedId,
        /// The repeated observation id.
        id: ObservationId,
    },
    /// The observation names a feed that is neither the authenticated peer's own nor
    /// delegated to it. [VLD-ANB-MIDGARD-0041]
    NotFromPeer {
        /// The feed the observation names.
        feed: FeedId,
        /// The authenticated PQ-mTLS peer that submitted it.
        peer: ServiceId,
    },
}

impl From<CryptoError> for FeedError {
    fn from(e: CryptoError) -> Self {
        FeedError::Crypto(format!("{e:?}"))
    }
}

impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedError::UnknownFeed(id) => write!(f, "unknown feed '{}'", id.as_str()),
            FeedError::AlgorithmMismatch { expected, got } => write!(
                f,
                "signature algorithm {} does not match feed key algorithm {}",
                got.as_str(),
                expected.as_str()
            ),
            FeedError::SignatureInvalid(id) => {
                write!(f, "signature failed to verify for feed '{}'", id.as_str())
            }
            FeedError::Crypto(e) => write!(f, "crypto backend error: {e}"),
            FeedError::SequenceUnset(id) => {
                write!(f, "observation from feed '{}' has no sequence", id.as_str())
            }
            FeedError::SequenceNotMonotonic { feed, got, last } => write!(
                f,
                "observation sequence {got} from feed '{}' is not after the last admitted                  sequence {last}",
                feed.as_str()
            ),
            FeedError::Stale {
                feed,
                observed_at,
                now,
            } => write!(
                f,
                "observation from feed '{}' dated {observed_at} is outside the freshness                  window at {now}",
                feed.as_str()
            ),
            FeedError::Replayed { feed, id } => write!(
                f,
                "observation '{}' from feed '{}' has already been received",
                id.0,
                feed.as_str()
            ),
            FeedError::NotFromPeer { feed, peer } => write!(
                f,
                "peer '{}' may not submit observations for feed '{}'",
                peer.0,
                feed.as_str()
            ),
        }
    }
}

impl std::error::Error for FeedError {}
