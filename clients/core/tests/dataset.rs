//! The demo dataset is accepted only under its own domain, with a pinned key, for the
//! release it names; any tamper is refused. [NFR-PQ-3] [AC-C-13]

use attest::accept::{preimage, AcceptError, BIFROST_ARTEFACT, DEMO_DATASET, RELEASE_ARTEFACT};
use attest::envelope::{self, SigEnvelope};
use attest::{SignError, SigningBackend, TrustAnchors};
use midgard_client_core::demo::dataset::{self, DatasetError};
use pqc_agility::backend::fips::provider;
use pqc_agility::{OsRng, SigId, Signature, SigningKey, VerifyingKey};

struct TestSigner {
    id: String,
    alg: SigId,
    sk: SigningKey,
    vk: VerifyingKey,
}
impl TestSigner {
    fn new(id: &str) -> Self {
        Self::with_alg(id, SigId::MlDsa65)
    }
    fn with_alg(id: &str, alg: SigId) -> Self {
        let (vk, sk) = provider()
            .sig_for(alg)
            .unwrap()
            .keypair(&mut OsRng)
            .unwrap();
        Self {
            id: id.into(),
            alg,
            sk,
            vk,
        }
    }
    fn anchors(&self) -> TrustAnchors {
        let mut a = TrustAnchors::new();
        a.insert(self.id.clone(), self.vk.clone());
        a
    }
}
impl SigningBackend for TestSigner {
    fn key_id(&self) -> &str {
        &self.id
    }
    fn sign(&self, msg: &[u8]) -> Result<Signature, SignError> {
        provider()
            .sig_for(self.alg)
            .unwrap()
            .sign(&self.sk, msg)
            .map_err(|e| SignError::Custodian(e.to_string()))
    }
    fn verifying_key(&self) -> &VerifyingKey {
        &self.vk
    }
}

const RELEASE: &str = "0.1.0-rc.3";
const BODY: &[u8] = br#"{"format":"midgard-demo-dataset","version":1}"#;

/// A file whose envelope is assembled by hand: `domain_label` goes into the envelope,
/// `signed_under` is the domain whose preimage the signature actually covers.
fn hand_made(
    s: &TestSigner,
    release: &str,
    body: &[u8],
    domain_label: &[u8],
    signed_under: &attest::Domain,
) -> Vec<u8> {
    let mut file = dataset::signed_bytes(release, body).unwrap();
    let signature = s.sign(&preimage(signed_under, &file)).unwrap();
    let env = SigEnvelope {
        domain: domain_label.to_vec(),
        key_id: s.key_id().to_string(),
        signature,
    }
    .encode();
    file.extend_from_slice(&(env.len() as u32).to_le_bytes());
    file.extend_from_slice(&env);
    file
}

/// Where the envelope starts (after its 4-byte length).
fn envelope_at() -> usize {
    dataset::signed_bytes(RELEASE, BODY).unwrap().len() + 4
}

#[test]
fn a_signed_dataset_round_trips() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    let ds = dataset::verify(&file, &s.anchors(), RELEASE).unwrap();
    assert_eq!(ds.body_bytes, BODY);
    assert_eq!(ds.key_id, "demo-test");
    assert_eq!(ds.release, RELEASE);
    assert_eq!(
        ds.body.get("format").and_then(|v| v.as_str()),
        Some("midgard-demo-dataset")
    );
}

#[test]
fn ml_dsa_87_is_accepted_too() {
    let s = TestSigner::with_alg("ceremony", SigId::MlDsa87);
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    dataset::verify(&file, &s.anchors(), RELEASE).unwrap();
}

#[test]
fn any_flipped_byte_is_refused() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    for i in [0, 8, 12, 30, file.len() - 1] {
        let mut bad = file.clone();
        bad[i] ^= 0x01;
        assert!(
            dataset::verify(&bad, &s.anchors(), RELEASE).is_err(),
            "byte {i} accepted"
        );
    }
}

#[test]
fn a_flipped_byte_anywhere_in_the_signed_part_or_envelope_header_is_refused() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    // Every byte of magic, version, release, body and the envelope's own header:
    // MGSIG1 magic, version, algorithm tag, then length-prefixed domain and key id,
    // then the signature's length prefix (the signature itself is checked elsewhere).
    const ENV_VERSION: usize = 1;
    const ENV_ALG_TAG: usize = 1;
    const LEN_PREFIX: usize = 4;
    let header_end = envelope_at()
        + envelope::MAGIC.len()
        + ENV_VERSION
        + ENV_ALG_TAG
        + LEN_PREFIX
        + DEMO_DATASET.label().len()
        + LEN_PREFIX
        + s.key_id().len()
        + LEN_PREFIX;
    for i in 0..header_end {
        let mut bad = file.clone();
        bad[i] ^= 0x01;
        assert!(
            dataset::verify(&bad, &s.anchors(), RELEASE).is_err(),
            "byte {i} accepted"
        );
    }
}

#[test]
fn a_flipped_byte_in_the_body_is_a_signature_refusal() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    let body_at = dataset::signed_bytes(RELEASE, BODY).unwrap().len() - BODY.len();
    for off in [0, 5, BODY.len() - 1] {
        let mut bad = file.clone();
        bad[body_at + off] ^= 0x01;
        assert!(
            matches!(
                dataset::verify(&bad, &s.anchors(), RELEASE),
                Err(DatasetError::Signature(AcceptError::BadSignature))
            ),
            "body byte {off}"
        );
    }
}

#[test]
fn a_flipped_byte_in_the_signature_is_a_signature_refusal() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    // The signature is the envelope's last field, so it ends the file.
    let sig_len = s.sign(b"x").unwrap().as_bytes().len();
    let sig_at = file.len() - sig_len;
    for off in (0..sig_len).step_by(97).chain([sig_len - 1]) {
        let mut bad = file.clone();
        bad[sig_at + off] ^= 0x80;
        assert!(
            matches!(
                dataset::verify(&bad, &s.anchors(), RELEASE),
                Err(DatasetError::Signature(AcceptError::BadSignature))
            ),
            "signature byte {off}"
        );
    }
}

#[test]
fn every_truncation_is_malformed() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    for n in 0..file.len() {
        assert!(
            matches!(
                dataset::verify(&file[..n], &s.anchors(), RELEASE),
                Err(DatasetError::Malformed(_))
            ),
            "a {n}-byte prefix was not refused as malformed"
        );
    }
}

#[test]
fn an_unpinned_key_another_release_or_trailing_bytes_are_refused() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    let other = TestSigner::new("demo-test");
    assert!(matches!(
        dataset::verify(&file, &other.anchors(), RELEASE),
        Err(DatasetError::Signature(_))
    ));
    assert!(matches!(
        dataset::verify(&file, &TrustAnchors::new(), RELEASE),
        Err(DatasetError::UnknownKey(_))
    ));
    let elsewhere = TestSigner::new("demo-other");
    assert!(matches!(
        dataset::verify(&file, &elsewhere.anchors(), RELEASE),
        Err(DatasetError::UnknownKey(_))
    ));
    assert!(matches!(
        dataset::verify(&file, &s.anchors(), "0.1.0-rc.4"),
        Err(DatasetError::WrongRelease { .. })
    ));
    let mut long = file.clone();
    long.push(0);
    assert!(matches!(
        dataset::verify(&long, &s.anchors(), RELEASE),
        Err(DatasetError::Malformed(_))
    ));
}

#[test]
fn a_key_of_another_algorithm_under_the_pinned_id_is_refused() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    let other = TestSigner::with_alg("demo-test", SigId::MlDsa87);
    assert!(matches!(
        dataset::verify(&file, &other.anchors(), RELEASE),
        Err(DatasetError::Signature(
            AcceptError::AlgorithmMismatch { .. }
        ))
    ));
}

#[test]
fn an_unapproved_algorithm_is_refused_to_sign_and_to_verify() {
    let weak = TestSigner::with_alg("demo-test", SigId::MlDsa44);
    assert!(matches!(
        dataset::sign(RELEASE, BODY, &weak),
        Err(DatasetError::Sign(_))
    ));
    let file = hand_made(&weak, RELEASE, BODY, DEMO_DATASET.label(), &DEMO_DATASET);
    assert!(matches!(
        dataset::verify(&file, &weak.anchors(), RELEASE),
        Err(DatasetError::Signature(
            AcceptError::UnapprovedAlgorithm { .. }
        ))
    ));
}

#[test]
fn another_signing_domain_is_refused() {
    let s = TestSigner::new("demo-test");
    // Labelled and signed as a Bifrost artefact.
    let relabelled = hand_made(
        &s,
        RELEASE,
        BODY,
        BIFROST_ARTEFACT.label(),
        &BIFROST_ARTEFACT,
    );
    assert!(matches!(
        dataset::verify(&relabelled, &s.anchors(), RELEASE),
        Err(DatasetError::WrongDomain)
    ));
    // Labelled as a demo dataset, but the signature covers a release-artefact preimage.
    let cross = hand_made(&s, RELEASE, BODY, DEMO_DATASET.label(), &RELEASE_ARTEFACT);
    assert!(matches!(
        dataset::verify(&cross, &s.anchors(), RELEASE),
        Err(DatasetError::Signature(AcceptError::BadSignature))
    ));
    // The hand-made path itself is sound: the right domain verifies.
    let good = hand_made(&s, RELEASE, BODY, DEMO_DATASET.label(), &DEMO_DATASET);
    dataset::verify(&good, &s.anchors(), RELEASE).unwrap();
}

#[test]
fn another_file_version_or_magic_is_refused() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    let mut v2 = file.clone();
    v2[dataset::MAGIC.len()] = 2;
    assert!(matches!(
        dataset::verify(&v2, &s.anchors(), RELEASE),
        Err(DatasetError::Malformed(_))
    ));
    let mut magic = file.clone();
    magic[6] = b'2';
    assert!(matches!(
        dataset::verify(&magic, &s.anchors(), RELEASE),
        Err(DatasetError::Malformed(_))
    ));
}

#[test]
fn the_release_label_cannot_be_swapped_under_a_valid_signature() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    // Same length, so every offset still parses: only the signature can catch it.
    let mut swapped = file.clone();
    let at = dataset::MAGIC.len() + 1 + 4;
    swapped[at..at + RELEASE.len()].copy_from_slice(b"0.1.0-rc.4");
    assert!(matches!(
        dataset::verify(&swapped, &s.anchors(), "0.1.0-rc.4"),
        Err(DatasetError::Signature(AcceptError::BadSignature))
    ));
}

#[test]
fn a_body_that_is_not_canonical_json_is_refused_even_when_signed() {
    let s = TestSigner::new("demo-test");
    for body in [
        &br#"{"version":1,"format":"midgard-demo-dataset"}"#[..],
        &br#"{"format": "midgard-demo-dataset","version":1}"#[..],
        &br#"{"format":"midgard-demo-dataset","version":1,"version":2}"#[..],
        &br#"{"format":"midgard-demo-dataset""#[..],
        &b"\xff"[..],
    ] {
        assert!(
            matches!(dataset::sign(RELEASE, body, &s), Err(DatasetError::Body(_))),
            "signed {body:?}"
        );
        let file = hand_made(&s, RELEASE, body, DEMO_DATASET.label(), &DEMO_DATASET);
        assert!(
            matches!(
                dataset::verify(&file, &s.anchors(), RELEASE),
                Err(DatasetError::Body(_))
            ),
            "accepted {body:?}"
        );
    }
}

#[test]
fn an_unpinned_key_id_is_echoed_only_as_a_bounded_prefix() {
    let long = "k".repeat(10_000);
    let s = TestSigner::new(&long);
    let file = dataset::sign(RELEASE, BODY, &s).unwrap();
    let err = dataset::verify(&file, &TrustAnchors::new(), RELEASE).unwrap_err();
    assert!(matches!(err, DatasetError::UnknownKey(_)));
    let shown = err.to_string();
    assert!(
        shown.contains(&"k".repeat(dataset::KEY_ID_SHOWN)),
        "{shown}"
    );
    assert!(
        !shown.contains(&"k".repeat(dataset::KEY_ID_SHOWN + 1)),
        "the message echoes more than {} characters of the key id",
        dataset::KEY_ID_SHOWN
    );
    // A short id is shown whole, with no ellipsis.
    let short = TestSigner::new("demo-other");
    let file = dataset::sign(RELEASE, BODY, &short).unwrap();
    let shown = dataset::verify(&file, &TrustAnchors::new(), RELEASE)
        .unwrap_err()
        .to_string();
    assert_eq!(shown, "signing key \"demo-other\" is not pinned");
}
