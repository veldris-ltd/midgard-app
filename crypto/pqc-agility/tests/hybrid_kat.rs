//! Known-answer tests for the hybrid combiner, `combine_hybrid` (X-Wing,
//! draft-connolly-cfrg-xwing-kem). The expected secrets were computed once, outside
//! this crate, with Python's `hashlib.sha3_256` over the same inert inputs, so a fault
//! in the vendored `sha3` crate or in the combiner's input order fails here; the
//! formula test in `hybrid.rs` cannot catch the first, because it uses the same
//! `sha3` on both sides. Runs with either backend. [NFR-PQ-4] [MG-SEC-09]
//! [VLD-ANB-MIDGARD-0024] [AC-53.1]

use pqc_agility::hybrid::{combine_hybrid, XWING_1024_LABEL, XWING_LABEL};
use pqc_agility::KemSuiteId;
use sha3::{Digest as _, Sha3_256};

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Inert inputs: ss_M = 00..1f, ss_X = 20..3f, ct_X = 40..5f, pk_X = 60..7f.
fn inputs() -> ([u8; 32], [u8; 32], [u8; 32], [u8; 32]) {
    let block = |start: u8| core::array::from_fn(|i| start + i as u8);
    (block(0x00), block(0x20), block(0x40), block(0x60))
}

#[test]
fn sha3_256_matches_fips_202() {
    // FIPS 202 / NIST CAVP SHA3-256 short-message answers for "" and "abc".
    assert_eq!(
        hex(&Sha3_256::digest(b"")),
        "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"
    );
    assert_eq!(
        hex(&Sha3_256::digest(b"abc")),
        "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
    );
}

#[test]
fn combiner_pinned_vector() {
    let (ss_m, ss_x, ct_x, pk_x) = inputs();
    for (suite, label, expected) in [
        (
            KemSuiteId::HybridX25519MlKem768,
            &XWING_LABEL[..],
            "0acca09f2fb739bc89668dbcd01ae5aebf9b72c6fe013297e3baa96854468491",
        ),
        (
            KemSuiteId::HybridX25519MlKem1024,
            XWING_1024_LABEL,
            "22af9aebdaf86b1774dc416134915490acf8283198d05faf2475c86bff421401",
        ),
    ] {
        assert_eq!(pqc_agility::hybrid::combiner_label(suite), label);
        let ss = combine_hybrid(suite, &ss_m, &ss_x, &ct_x, &pk_x);
        assert_eq!(hex(ss.expose()), expected, "{suite:?}");
    }
}

#[test]
fn changing_ct_or_pk_changes_the_secret() {
    let suite = KemSuiteId::HybridX25519MlKem768;
    let (ss_m, ss_x, ct_x, pk_x) = inputs();
    let base = *combine_hybrid(suite, &ss_m, &ss_x, &ct_x, &pk_x).expose();
    for i in 0..32 {
        let mut ct = ct_x;
        ct[i] ^= 0x01;
        assert_ne!(
            &base,
            combine_hybrid(suite, &ss_m, &ss_x, &ct, &pk_x).expose(),
            "ct_X byte {i}"
        );
        let mut pk = pk_x;
        pk[i] ^= 0x01;
        assert_ne!(
            &base,
            combine_hybrid(suite, &ss_m, &ss_x, &ct_x, &pk).expose(),
            "pk_X byte {i}"
        );
    }
    // The original inputs still give the pinned answer.
    assert_eq!(
        hex(&base),
        "0acca09f2fb739bc89668dbcd01ae5aebf9b72c6fe013297e3baa96854468491"
    );
}
