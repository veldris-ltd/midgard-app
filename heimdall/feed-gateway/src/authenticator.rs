//! The authenticator: turns a [`SignedObservation`] into a [`VerifiedObservation`]
//! iff its post-quantum source signature checks out. This is the single admission
//! gate to the picture. [FR-H-3] [NFR-PQ-3] [AC-H-2]
//!
//! Once the signature verifies, the observation must also be new: its feed's
//! sequence must advance, its `observed_at` must be fresh and its id unseen
//! ([`ReplayGuard`]). [VLD-ANB-MIDGARD-0041]

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use pq_mtls::ServiceId;
use pqc_agility::{SharedProvider, Signature, SignatureScheme, SigningKey};

use heimdall_picture::Observation;

use crate::error::FeedError;
use crate::registry::FeedRegistry;
use crate::replay::{FreshnessPolicy, ReplayGuard};

/// An observation as it arrives on the wire: the datum plus the feed's detached PQ
/// signature over its canonical bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct SignedObservation {
    /// The observation payload.
    pub observation: Observation,
    /// ML-DSA / SLH-DSA signature over `observation.canonical_bytes()`.
    pub signature: Signature,
}

/// An observation whose feed signature has been verified. It cannot be constructed
/// outside this crate, so possessing one is proof the source was authenticated —
/// the fusion engine consumes only this type. [AC-H-2]
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedObservation(Observation);

impl VerifiedObservation {
    /// Borrow the underlying observation.
    pub fn observation(&self) -> &Observation {
        &self.0
    }

    /// Consume into the underlying observation.
    pub fn into_observation(self) -> Observation {
        self.0
    }
}

/// Produce a [`SignedObservation`] by signing an observation's canonical bytes.
/// This is what a trusted feed source (or a feed SDK) does before transmitting; the
/// observation carries its sequence from a [`crate::FeedSequencer`].
pub fn sign_observation(
    scheme: &dyn SignatureScheme,
    signing_key: &SigningKey,
    observation: Observation,
) -> Result<SignedObservation, FeedError> {
    let signature = scheme.sign(signing_key, &observation.canonical_bytes())?;
    Ok(SignedObservation {
        observation,
        signature,
    })
}

/// The gateway's clock: Unix seconds.
type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

fn system_clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The feed gateway: a trust registry plus the shared crypto provider used to
/// resolve a verification scheme per signature algorithm, and the replay state,
/// clock and freshness window an authenticated observation is checked against.
pub struct FeedGateway {
    registry: FeedRegistry,
    provider: SharedProvider,
    replay: Arc<ReplayGuard>,
    clock: Clock,
    policy: FreshnessPolicy,
}

impl FeedGateway {
    /// Build a gateway over a feed registry and the shared agility provider, with its
    /// own empty [`ReplayGuard`], the system clock and the default
    /// [`FreshnessPolicy`].
    pub fn new(registry: FeedRegistry, provider: SharedProvider) -> Self {
        Self {
            registry,
            provider,
            replay: Arc::new(ReplayGuard::new()),
            clock: Arc::new(system_clock),
            policy: FreshnessPolicy::default(),
        }
    }

    /// Builder: share `replay` with other gateways (one per request or connection), so
    /// sequence and window state outlive each of them. [VLD-ANB-MIDGARD-0041]
    pub fn with_replay_guard(mut self, replay: Arc<ReplayGuard>) -> Self {
        self.replay = replay;
        self
    }

    /// Builder: the clock (Unix seconds) freshness is judged against.
    pub fn with_clock(mut self, clock: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// Builder: the freshness window.
    pub fn with_freshness(mut self, policy: FreshnessPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// The replay state this gateway records into.
    pub fn replay_guard(&self) -> &Arc<ReplayGuard> {
        &self.replay
    }

    /// Mutable access to the trust registry (register feeds here).
    pub fn registry_mut(&mut self) -> &mut FeedRegistry {
        &mut self.registry
    }

    /// The trust registry.
    pub fn registry(&self) -> &FeedRegistry {
        &self.registry
    }

    /// Verify a signed observation, admitting it to the picture iff its source
    /// signature checks out and it is new. Fails closed on an unknown feed, an
    /// algorithm mismatch, or an invalid signature, none of which yield a
    /// [`VerifiedObservation`] or touch the replay state, and then on an unset or
    /// non-monotonic sequence, a stale `observed_at` or a repeated id.
    /// [FR-H-3] [NFR-PQ-3] [AC-H-2] [VLD-ANB-MIDGARD-0041]
    pub fn authenticate(
        &self,
        signed: &SignedObservation,
    ) -> Result<VerifiedObservation, FeedError> {
        self.verify_signature(signed)?;
        // Only now, with the source proven, may the observation consume replay state.
        let o = &signed.observation;
        self.replay.check_and_record(
            &o.feed_id,
            &o.id,
            o.sequence,
            o.observed_at,
            (self.clock)(),
            &self.policy,
        )?;
        Ok(VerifiedObservation(o.clone()))
    }

    /// [`FeedGateway::authenticate`] for an observation received from the
    /// authenticated PQ-mTLS peer `peer`: the feed it names must be the peer's own
    /// (its service id) or delegated to the peer in the registry, or it is refused as
    /// [`FeedError::NotFromPeer`] before anything else is checked.
    /// [VLD-ANB-MIDGARD-0041]
    pub fn authenticate_from(
        &self,
        signed: &SignedObservation,
        peer: &ServiceId,
    ) -> Result<VerifiedObservation, FeedError> {
        let feed = &signed.observation.feed_id;
        if !self.registry.may_submit(peer, feed) {
            return Err(FeedError::NotFromPeer {
                feed: feed.clone(),
                peer: peer.clone(),
            });
        }
        self.authenticate(signed)
    }

    /// The signature check alone: registered feed, matching algorithm, valid
    /// signature over the canonical bytes.
    fn verify_signature(&self, signed: &SignedObservation) -> Result<(), FeedError> {
        let feed = &signed.observation.feed_id;
        let key = self
            .registry
            .key_for(feed)
            .ok_or_else(|| FeedError::UnknownFeed(feed.clone()))?;

        // The signature's algorithm must match the registered key's algorithm; a
        // mismatch is an attempt to substitute a weaker/foreign scheme.
        if signed.signature.alg() != key.alg() {
            return Err(FeedError::AlgorithmMismatch {
                expected: key.alg(),
                got: signed.signature.alg(),
            });
        }

        let provider = self.provider.read().expect("crypto provider lock poisoned");
        let scheme = provider.sig_for(key.alg())?;

        scheme
            .verify(
                key,
                &signed.observation.canonical_bytes(),
                &signed.signature,
            )
            .map_err(|_| FeedError::SignatureInvalid(feed.clone()))
    }
}
