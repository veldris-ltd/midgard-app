//! Negotiation is fail-closed and preference-ordered (AC-M-1). No backend needed.

use pqc_agility::registry::DEFAULT_KEM_PREFERENCE;
use pqc_agility::{negotiate_kem, KemSuiteId, NegotiationError};

#[test]
fn prefers_higher_security_when_no_hybrid() {
    // Peer offers only pure suites; local default prefers hybrids (absent), then
    // ML-KEM-1024 over ML-KEM-768.
    let peer = vec![KemSuiteId::MlKem768, KemSuiteId::MlKem1024];
    let sel = negotiate_kem(&DEFAULT_KEM_PREFERENCE, &peer).unwrap();
    assert_eq!(sel, KemSuiteId::MlKem1024);
}

#[test]
fn prefers_hybrid_when_offered() {
    let peer = vec![KemSuiteId::MlKem1024, KemSuiteId::HybridX25519MlKem768];
    let sel = negotiate_kem(&DEFAULT_KEM_PREFERENCE, &peer).unwrap();
    assert_eq!(sel, KemSuiteId::HybridX25519MlKem768);
}

#[test]
fn empty_peer_fails_closed() {
    let err = negotiate_kem(&DEFAULT_KEM_PREFERENCE, &[]).unwrap_err();
    assert_eq!(err, NegotiationError::EmptyPeerSet);
}

#[test]
fn no_overlap_fails_closed() {
    let local = [KemSuiteId::MlKem1024];
    let peer = [KemSuiteId::MlKem768];
    let err = negotiate_kem(&local, &peer).unwrap_err();
    assert_eq!(err, NegotiationError::NoCommonAlgorithm);
}
