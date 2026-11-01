//! Workload certificates, trust anchors and revocation lists on disk. A stored
//! certificate and anchor decode to exactly what was encoded and still verify; the
//! certificate's validity window is enforced; revocations travel only as a CA-signed,
//! versioned list that cannot be forged or rolled back and is picked up live; and a
//! malformed file is refused. [NFR-SEC-1] [RC-35] [MG-SEC-02] [MG-SEC-03]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::time::Duration;

use policy::{ClassificationLevel, Role};
use pq_mtls::{
    cert_fingerprint, certify, decode_anchor, decode_cert, decode_crl, encode_anchor, encode_cert,
    encode_crl, reload_crl_file, spawn_crl_reloader, verify_cert, CertTerms, IdentityCert,
    MtlsError, Posture, RevocationList, ServiceId, TrustAnchor, WorkloadIdentity,
};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{OsRng, SigId, SignatureScheme, SigningKey};

const ALG: SigId = SigId::MlDsa65;
const NOW: u64 = 1_800_000_000;
const DAY: u64 = 86_400;

struct Issued {
    cert: IdentityCert,
    anchor: TrustAnchor,
    ca_sk: SigningKey,
}

fn issued() -> Issued {
    let scheme = FipsSig::new(ALG);
    let (ca_vk, ca_sk) = scheme.keypair(&mut OsRng).unwrap();
    let (workload_vk, _) = scheme.keypair(&mut OsRng).unwrap();
    let identity = WorkloadIdentity {
        id: ServiceId("mimir-server".into()),
        roles: BTreeSet::from([Role("inference-server".into())]),
        sig_alg: ALG,
        verifying_key: workload_vk,
    };
    let terms = CertTerms::new(
        7,
        NOW,
        NOW + 30 * DAY,
        Posture {
            level: ClassificationLevel::SECRET,
            compartments: BTreeSet::new(),
        },
    );
    let cert = certify(&fips_provider(), ALG, &ca_sk, &identity, terms).unwrap();
    Issued {
        cert,
        anchor: TrustAnchor::new(ALG, ca_vk),
        ca_sk,
    }
}

fn crl(i: &Issued, version: u64, revoked: &[[u8; 32]]) -> RevocationList {
    RevocationList::sign(
        &fips_provider(),
        ALG,
        &i.ca_sk,
        version,
        NOW,
        revoked.iter().copied().collect(),
    )
    .unwrap()
}

#[test]
fn a_stored_certificate_and_anchor_still_verify() {
    let i = issued();
    let cert_back = decode_cert(&encode_cert(&i.cert)).expect("the certificate decodes");
    let anchor_back = decode_anchor(&encode_anchor(&i.anchor)).expect("the anchor decodes");
    assert_eq!(cert_back, i.cert);
    assert_eq!(anchor_back, i.anchor);
    verify_cert(&cert_back, &anchor_back, &fips_provider(), NOW + DAY).expect("still verifies");
}

/// MG-SEC-02: the validity window is part of the signed certificate and is checked.
#[test]
fn a_certificate_only_works_inside_its_validity_window() {
    let i = issued();
    let p = fips_provider();
    verify_cert(&i.cert, &i.anchor, &p, NOW).expect("valid from not_before");
    for at in [NOW - 1, NOW + 30 * DAY, NOW + 365 * DAY] {
        assert!(
            matches!(
                verify_cert(&i.cert, &i.anchor, &p, at),
                Err(MtlsError::PeerCertExpired { .. })
            ),
            "a certificate must not verify at {at}"
        );
    }
}

/// MG-SEC-02/04: the terms are signed — stretching the window or raising the
/// clearance on a copied certificate breaks the CA signature.
#[test]
fn the_terms_cannot_be_edited() {
    let i = issued();
    let p = fips_provider();
    let mut longer = i.cert.clone();
    longer.terms.not_after = u64::MAX;
    let mut higher = i.cert.clone();
    higher.terms.clearance.level = ClassificationLevel::TOP_SECRET;
    for forged in [longer, higher] {
        assert!(matches!(
            verify_cert(&forged, &i.anchor, &p, NOW + DAY),
            Err(MtlsError::PeerCertUntrusted)
        ));
    }
}

/// MG-SEC-03: revocation arrives as a CA-signed list; once installed the cert is
/// refused. The anchor file itself carries no revocations.
#[test]
fn a_signed_revocation_list_revokes() {
    let i = issued();
    let p = fips_provider();
    let list = decode_crl(&encode_crl(&crl(&i, 1, &[cert_fingerprint(&i.cert)]))).unwrap();
    assert!(i.anchor.install_crl(&list, &p).unwrap());
    assert_eq!(i.anchor.crl_version(), 1);
    assert!(matches!(
        verify_cert(&i.cert, &i.anchor, &p, NOW + DAY),
        Err(MtlsError::PeerCertRevoked)
    ));
    let back = decode_anchor(&encode_anchor(&i.anchor)).unwrap();
    assert!(
        back.revoked().is_empty(),
        "revocations never travel unsigned"
    );
}

/// MG-SEC-03: a list the CA did not sign, or an older one, is refused and changes
/// nothing.
#[test]
fn a_forged_or_older_revocation_list_is_refused() {
    let i = issued();
    let p = fips_provider();
    let fp = cert_fingerprint(&i.cert);

    // Signed by someone else.
    let (_, other_sk) = FipsSig::new(ALG).keypair(&mut OsRng).unwrap();
    let forged = RevocationList::sign(&p, ALG, &other_sk, 9, NOW, BTreeSet::new()).unwrap();
    assert!(matches!(
        i.anchor.install_crl(&forged, &p),
        Err(MtlsError::CrlUntrusted)
    ));
    // Signed, then edited.
    let mut edited = crl(&i, 2, &[fp]);
    edited.revoked.clear();
    assert!(matches!(
        i.anchor.install_crl(&edited, &p),
        Err(MtlsError::CrlUntrusted)
    ));

    // Version 2 revokes; restoring the signed version 1 (which did not) is refused.
    i.anchor.install_crl(&crl(&i, 2, &[fp]), &p).unwrap();
    assert!(matches!(
        i.anchor.install_crl(&crl(&i, 1, &[]), &p),
        Err(MtlsError::CrlRollback {
            installed: 2,
            offered: 1
        })
    ));
    assert!(i.anchor.is_revoked(&fp), "the revocation stands");
}

/// MG-SEC-03: a newer list on disk reaches every clone of the anchor while the
/// process runs — no restart.
#[test]
fn a_newer_list_on_disk_is_picked_up_live() {
    let i = issued();
    let p = fips_provider();
    let dir = std::env::temp_dir().join(format!("mg-crl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mesh.crl");
    std::fs::write(&path, encode_crl(&crl(&i, 1, &[]))).unwrap();
    assert!(reload_crl_file(&i.anchor, &path, &p).unwrap());

    // A clone handed to a connection before the revocation.
    let per_connection = i.anchor.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    spawn_crl_reloader(
        i.anchor.clone(),
        path.clone(),
        Duration::from_millis(20),
        fips_provider(),
        move |r| {
            let _ = tx.send(r.is_ok());
        },
    );
    std::thread::sleep(Duration::from_millis(60));
    std::fs::write(&path, encode_crl(&crl(&i, 2, &[cert_fingerprint(&i.cert)]))).unwrap();
    assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    assert!(matches!(
        verify_cert(&i.cert, &per_connection, &p, NOW + DAY),
        Err(MtlsError::PeerCertRevoked)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_malformed_file_is_refused() {
    let i = issued();
    let cert_bytes = encode_cert(&i.cert);
    let anchor_bytes = encode_anchor(&i.anchor);
    let crl_bytes = encode_crl(&crl(&i, 1, &[[1; 32]]));

    assert!(
        decode_cert(&anchor_bytes).is_err(),
        "an anchor is not a certificate"
    );
    assert!(
        decode_anchor(&cert_bytes).is_err(),
        "a certificate is not an anchor"
    );
    assert!(
        decode_crl(&anchor_bytes).is_err(),
        "an anchor is not a list"
    );
    for (what, bytes) in [
        ("cert", &cert_bytes),
        ("anchor", &anchor_bytes),
        ("crl", &crl_bytes),
    ] {
        let short = &bytes[..bytes.len() - 1];
        let mut long = bytes.clone();
        long.push(0);
        let refused = |b: &[u8]| match what {
            "cert" => decode_cert(b).is_err(),
            "anchor" => decode_anchor(b).is_err(),
            _ => decode_crl(b).is_err(),
        };
        assert!(refused(short), "{what}: truncated");
        assert!(refused(&long), "{what}: trailing bytes");
    }

    // A version-1 file (unsigned revocations beside the key) is refused outright.
    let mut v1 = anchor_bytes.clone();
    v1[8] = b'1';
    assert!(decode_anchor(&v1).is_err());

    // A hostile revocation count cannot make decoding allocate or loop unboundedly.
    let mut hostile = crl_bytes;
    hostile[22..26].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_crl(&hostile).is_err());
}
