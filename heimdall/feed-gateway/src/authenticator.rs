//! The authenticator: turns a [`SignedObservation`] into a [`VerifiedObservation`]
//! iff its post-quantum source signature checks out. This is the single admission
//! gate to the picture. [FR-H-3] [NFR-PQ-3] [AC-H-2]

use pqc_agility::{SharedProvider, Signature, SignatureScheme, SigningKey};

use heimdall_picture::Observation;

use crate::error::FeedError;
use crate::registry::FeedRegistry;

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
/// This is what a trusted feed source (or a feed SDK) does before transmitting.
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

/// The feed gateway: a trust registry plus the shared crypto provider used to
/// resolve a verification scheme per signature algorithm.
pub struct FeedGateway {
    registry: FeedRegistry,
    provider: SharedProvider,
}

impl FeedGateway {
    /// Build a gateway over a feed registry and the shared agility provider.
    pub fn new(registry: FeedRegistry, provider: SharedProvider) -> Self {
        Self { registry, provider }
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
    /// signature checks out. Fails closed on an unknown feed, an algorithm
    /// mismatch, or an invalid signature — none of which yield a
    /// [`VerifiedObservation`]. [FR-H-3] [NFR-PQ-3] [AC-H-2]
    pub fn authenticate(
        &self,
        signed: &SignedObservation,
    ) -> Result<VerifiedObservation, FeedError> {
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
            .map_err(|_| FeedError::SignatureInvalid(feed.clone()))?;

        Ok(VerifiedObservation(signed.observation.clone()))
    }
}
