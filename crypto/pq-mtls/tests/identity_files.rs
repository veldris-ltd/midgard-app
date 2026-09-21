//! Workload certificates and trust anchors on disk. A stored certificate and anchor
//! decode to exactly what was encoded, the certificate still verifies against the
//! anchor, a revocation travels with the anchor and still refuses the certificate,
//! and a malformed file is refused. [NFR-SEC-1] [RC-35]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;

use policy::Role;
use pq_mtls::{
    canonical_identity, cert_fingerprint, decode_anchor, decode_cert, encode_anchor, encode_cert,
    verify_cert, IdentityCert, MtlsError, ServiceId, TrustAnchor, WorkloadIdentity,
};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{OsRng, SigId, SignatureScheme};

const ALG: SigId = SigId::MlDsa65;

fn issued() -> (IdentityCert, TrustAnchor) {
    let scheme = FipsSig::new(ALG);
    let (ca_vk, ca_sk) = scheme.keypair(&mut OsRng).unwrap();
    let (workload_vk, _) = scheme.keypair(&mut OsRng).unwrap();
    let identity = WorkloadIdentity {
        id: ServiceId("mimir-server".into()),
        roles: BTreeSet::from([Role("inference-server".into())]),
        sig_alg: ALG,
        verifying_key: workload_vk,
    };
    let ca_signature = scheme.sign(&ca_sk, &canonical_identity(&identity)).unwrap();
    (
        IdentityCert {
            identity,
            ca_sig_alg: ALG,
            ca_signature,
        },
        TrustAnchor::new(ALG, ca_vk),
    )
}

#[test]
fn a_stored_certificate_and_anchor_still_verify() {
    let (cert, anchor) = issued();
    let cert_back = decode_cert(&encode_cert(&cert)).expect("the certificate decodes");
    let anchor_back = decode_anchor(&encode_anchor(&anchor)).expect("the anchor decodes");
    assert_eq!(cert_back, cert);
    assert_eq!(anchor_back, anchor);
    verify_cert(&cert_back, &anchor_back, &fips_provider()).expect("still verifies");
}

#[test]
fn a_revocation_travels_with_the_anchor() {
    let (cert, anchor) = issued();
    let revoked = anchor.with_revocations([cert_fingerprint(&cert)]);
    let back = decode_anchor(&encode_anchor(&revoked)).expect("decodes");
    assert!(back.is_revoked(&cert_fingerprint(&cert)));
    assert!(matches!(
        verify_cert(&cert, &back, &fips_provider()),
        Err(MtlsError::PeerCertRevoked)
    ));
}

#[test]
fn a_malformed_file_is_refused() {
    let (cert, anchor) = issued();
    let cert_bytes = encode_cert(&cert);
    let anchor_bytes = encode_anchor(&anchor);

    assert!(
        decode_cert(&anchor_bytes).is_err(),
        "an anchor is not a certificate"
    );
    assert!(
        decode_anchor(&cert_bytes).is_err(),
        "a certificate is not an anchor"
    );
    assert!(
        decode_cert(&cert_bytes[..cert_bytes.len() - 1]).is_err(),
        "truncated"
    );
    assert!(
        decode_anchor(&anchor_bytes[..anchor_bytes.len() - 1]).is_err(),
        "truncated"
    );

    let mut trailing = cert_bytes.clone();
    trailing.push(0);
    assert!(decode_cert(&trailing).is_err(), "trailing bytes");
    let mut trailing = anchor_bytes.clone();
    trailing.push(0);
    assert!(decode_anchor(&trailing).is_err(), "trailing bytes");

    // A hostile revocation count cannot make decoding allocate or loop unboundedly.
    let mut hostile = anchor_bytes;
    let count_at = hostile.len() - 4;
    hostile[count_at..].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_anchor(&hostile).is_err());
}
