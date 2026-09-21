//! Algorithm negotiation. Picks the highest local-preference suite the peer also
//! offers, and **fails closed** when there is no common post-quantum suite
//! (AC-M-1). Because [`KemSuiteId`] enumerates only PQ/hybrid suites, there is no
//! code path that can select a classical-only suite.

use crate::algorithm::{KemSuiteId, SigId};
use crate::error::NegotiationError;

/// A party's offered capabilities, in descending preference order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Offered KEM suites, most preferred first.
    pub kem_suites: Vec<KemSuiteId>,
    /// Offered signature algorithms, most preferred first.
    pub sig_algs: Vec<SigId>,
}

/// Select a KEM suite from the local preference list that the peer also offers.
///
/// - Empty peer list → [`NegotiationError::EmptyPeerSet`].
/// - No overlap → [`NegotiationError::NoCommonAlgorithm`].
/// - Otherwise the first (highest-preference) common, post-quantum suite.
pub fn negotiate_kem(
    local_pref: &[KemSuiteId],
    peer: &[KemSuiteId],
) -> Result<KemSuiteId, NegotiationError> {
    if peer.is_empty() {
        return Err(NegotiationError::EmptyPeerSet);
    }
    for &suite in local_pref {
        if suite.is_post_quantum() && peer.contains(&suite) {
            return Ok(suite);
        }
    }
    Err(NegotiationError::NoCommonAlgorithm)
}

/// Select a signature algorithm the same way.
pub fn negotiate_sig(local_pref: &[SigId], peer: &[SigId]) -> Result<SigId, NegotiationError> {
    if peer.is_empty() {
        return Err(NegotiationError::EmptyPeerSet);
    }
    for &alg in local_pref {
        if alg.is_post_quantum() && peer.contains(&alg) {
            return Ok(alg);
        }
    }
    Err(NegotiationError::NoCommonAlgorithm)
}
