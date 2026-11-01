//! Known-answer tests for the `hash` module: SHA-256 (NIST FIPS 180-4 / CAVP),
//! HMAC-SHA256 (RFC 4231 test cases 1–7), HKDF-SHA256 (RFC 5869 test cases 1–3) and
//! PBKDF2-HMAC-SHA256 (the widely published RFC 6070-style SHA-256 vectors, plus
//! RFC 7914 §11). The primitives come from the vetted RustCrypto crates; these
//! vectors prove the wrappers around them compute the standard functions — a
//! round-trip test cannot, because both ends would share any bug. Any error fails
//! CI loudly. [MG-SEC-10]

use pqc_agility::hash::{
    ct_eq, hkdf_sha256_32, hmac_sha256, pbkdf2_hmac_sha256, sha256, Digest, Sha256,
};

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len() % 2 == 0, "odd-length hex");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[test]
fn kat_empty() {
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn kat_abc() {
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn kat_448_bits() {
    let msg = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    assert_eq!(
        hex(&sha256(msg)),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn kat_million_a() {
    // Fed incrementally to exercise buffering across many blocks.
    let mut h = Sha256::new();
    let chunk = [b'a'; 1000];
    for _ in 0..1000 {
        h.update(&chunk);
    }
    assert_eq!(
        hex(&h.finalize()),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn kat_two_block_boundary() {
    // 56 bytes forces the length into a second padding block (edge of finalize).
    let msg = vec![b'x'; 56];
    let a = sha256(&msg);
    let mut h = Sha256::new();
    for b in &msg {
        h.update(&[*b]); // byte-at-a-time must match one-shot
    }
    assert_eq!(a, h.finalize());
}

#[test]
fn kat_clone_snapshots_a_running_hash() {
    // Transcript hashing snapshots a running hash by cloning it; the clone and the
    // original must then evolve independently.
    let mut h = Sha256::new();
    h.update(b"ab");
    let snapshot = h.clone();
    h.update(b"c");
    assert_eq!(
        hex(&h.finalize()),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(snapshot.finalize(), sha256(b"ab"));
}

// ---- HMAC-SHA256: RFC 4231 §4 ---------------------------------------------

/// (key, data, expected HMAC-SHA256 — truncated to 128 bits for test case 5).
fn rfc4231_cases() -> Vec<(Vec<u8>, Vec<u8>, &'static str)> {
    vec![
        // Test case 1
        (
            vec![0x0b; 20],
            b"Hi There".to_vec(),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
        ),
        // Test case 2: a key shorter than the output
        (
            b"Jefe".to_vec(),
            b"what do ya want for nothing?".to_vec(),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
        ),
        // Test case 3
        (
            vec![0xaa; 20],
            vec![0xdd; 50],
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
        ),
        // Test case 4
        (
            (0x01..=0x19).collect(),
            vec![0xcd; 50],
            "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b",
        ),
        // Test case 5: output truncated to 128 bits
        (
            vec![0x0c; 20],
            b"Test With Truncation".to_vec(),
            "a3b6167473100ee06e0c796c2955552b",
        ),
        // Test case 6: a key larger than the block size is hashed first
        (
            vec![0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First".to_vec(),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
        ),
        // Test case 7: large key and large data
        (
            vec![0xaa; 131],
            b"This is a test using a larger than block-size key and a larger than \
              block-size data. The key needs to be hashed before being used by the \
              HMAC algorithm."
                .to_vec(),
            "9b09ffa71b942fcb27635fbcd5b0e944bfdc63644f0713938a7f51535c3a35e2",
        ),
    ]
}

#[test]
fn kat_hmac_sha256_rfc4231() {
    for (i, (key, data, expected)) in rfc4231_cases().into_iter().enumerate() {
        let mac = hmac_sha256(&key, &data);
        let expected = unhex(expected);
        assert_eq!(
            &mac[..expected.len()],
            &expected[..],
            "RFC 4231 test case {}",
            i + 1
        );
    }
}

// ---- HKDF-SHA256: RFC 5869 Appendix A.1–A.3 --------------------------------

struct HkdfCase {
    ikm: Vec<u8>,
    salt: Vec<u8>,
    info: Vec<u8>,
    prk: &'static str,
    okm: &'static str,
}

fn rfc5869_cases() -> Vec<HkdfCase> {
    vec![
        // A.1 Basic test case
        HkdfCase {
            ikm: vec![0x0b; 22],
            salt: (0x00..=0x0c).collect(),
            info: (0xf0..=0xf9).collect(),
            prk: "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5",
            okm: "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf\
                  34007208d5b887185865",
        },
        // A.2 Longer inputs/outputs
        HkdfCase {
            ikm: (0x00..=0x4f).collect(),
            salt: (0x60..=0xaf).collect(),
            info: (0xb0..=0xff).collect(),
            prk: "06a6b88c5853361a06104c9ceb35b45cef760014904671014a193f40c15fc244",
            okm: "b11e398dc80327a1c8e7f78c596a49344f012eda2d4efad8a050cc4c19afa97c\
                  59045a99cac7827271cb41c65e590e09da3275600c2f09b8367793a9aca3db71\
                  cc30c58179ec3e87c14c01d5c1f3434f1d87",
        },
        // A.3 Zero-length salt and info
        HkdfCase {
            ikm: vec![0x0b; 22],
            salt: Vec::new(),
            info: Vec::new(),
            prk: "19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04",
            okm: "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d\
                  9d201395faa4b61a96c8",
        },
    ]
}

#[test]
fn kat_hkdf_sha256_rfc5869() {
    for (i, case) in rfc5869_cases().into_iter().enumerate() {
        let n = i + 1;
        // Extract: PRK = HMAC-Hash(salt, IKM). An empty salt is "not provided",
        // i.e. HashLen zero bytes, which HMAC's zero-padding makes identical.
        let salt_or_zeros = if case.salt.is_empty() {
            vec![0u8; 32]
        } else {
            case.salt.clone()
        };
        assert_eq!(
            hmac_sha256(&salt_or_zeros, &case.ikm).to_vec(),
            unhex(case.prk),
            "RFC 5869 test case {n}: PRK"
        );
        // Expand: `hkdf_sha256_32` yields L = 32, which is exactly the first 32 bytes
        // of the RFC's longer OKM (T(1) is the same whatever L is).
        let okm = unhex(case.okm);
        assert_eq!(
            hkdf_sha256_32(&case.salt, &case.ikm, &case.info).to_vec(),
            okm[..32].to_vec(),
            "RFC 5869 test case {n}: OKM"
        );
    }
}

// ---- PBKDF2-HMAC-SHA256 -----------------------------------------------------

#[test]
fn kat_pbkdf2_hmac_sha256() {
    // (password, salt, iterations, expected DK; dkLen = its length)
    let cases: [(&[u8], &[u8], u32, &str); 7] = [
        // RFC 6070's inputs, with the widely published HMAC-SHA256 outputs.
        (
            b"password",
            b"salt",
            1,
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b",
        ),
        (
            b"password",
            b"salt",
            2,
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43",
        ),
        (
            b"password",
            b"salt",
            4096,
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a",
        ),
        (
            b"passwordPASSWORDpassword",
            b"saltSALTsaltSALTsaltSALTsaltSALTsalt",
            4096,
            "348c89dbcbd32b2f32d814b8116e84cf2b17347ebc1800181c4e2a1fb8dd53e1c635518c7dac47e9",
        ),
        (
            b"pass\0word",
            b"sa\0lt",
            4096,
            "89b69d0516f829893c696226650a8687",
        ),
        // RFC 7914 §11 (dkLen = 64: two output blocks).
        (
            b"passwd",
            b"salt",
            1,
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc\
             49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783",
        ),
        (
            b"Password",
            b"NaCl",
            80_000,
            "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56\
             a1d425a1225833549adb841b51c9b3176a272bdebba1d078478f62b397f33c8d",
        ),
    ];
    for (password, salt, iterations, expected) in cases {
        let expected = unhex(expected);
        let mut dk = vec![0u8; expected.len()];
        pbkdf2_hmac_sha256(password, salt, iterations, &mut dk);
        assert_eq!(dk, expected, "PBKDF2 c={iterations} dkLen={}", dk.len());
    }
}

// ---- constant-time compare -------------------------------------------------

#[test]
fn ct_eq_is_equality_and_refuses_different_lengths() {
    let mac = hmac_sha256(b"key", b"msg");
    assert!(ct_eq(&mac, &hmac_sha256(b"key", b"msg")));
    assert!(!ct_eq(&mac, &hmac_sha256(b"key", b"msh")));
    // A truncated tag is not a match, whichever side is shorter.
    assert!(!ct_eq(&mac[..16], &mac));
    assert!(!ct_eq(&mac, &mac[..31]));
    assert!(ct_eq(b"", b""));
    assert!(!ct_eq(b"", b"\0"));
}
