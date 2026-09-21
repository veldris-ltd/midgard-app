//! AES-256-GCM known-answer + behavioural tests. The KAT vectors are the
//! canonical McGrew–Viega GCM test cases 13 & 14 (AES-256), so this anchors our
//! use of the primitive to published answers, not just to the upstream crate.
#![cfg(feature = "backend-fips")]

use pqc_agility::Aes256GcmAead;

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// GCM Test Case 13 (AES-256): all-zero key & IV, empty plaintext and AAD.
/// Expected output is the 16-byte tag only.
#[test]
fn kat_case13_empty_plaintext() {
    let aead = Aes256GcmAead::new(&[0u8; 32]);
    let out = aead.seal(&[0u8; 12], &[], &[]).unwrap();
    assert_eq!(hex(&out), "530f8afbc74536b9a963b4f1c4cb738b");
}

/// GCM Test Case 14 (AES-256): all-zero key & IV, 16 zero plaintext bytes, empty
/// AAD. Expected output is ciphertext ‖ tag.
#[test]
fn kat_case14_zero_block() {
    let aead = Aes256GcmAead::new(&[0u8; 32]);
    let out = aead.seal(&[0u8; 12], &[], &[0u8; 16]).unwrap();
    assert_eq!(
        hex(&out),
        "cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919"
    );
}

#[test]
fn roundtrip_with_aad() {
    let key = [7u8; 32];
    let nonce = [3u8; 12];
    let aad = b"record-header-v1";
    let msg = b"the quick brown fox jumps over the lazy dog";
    let aead = Aes256GcmAead::new(&key);
    let ct = aead.seal(&nonce, aad, msg).unwrap();
    assert_eq!(ct.len(), msg.len() + 16, "ciphertext carries a 16-byte tag");
    let pt = aead.open(&nonce, aad, &ct).unwrap();
    assert_eq!(pt, msg);
}

#[test]
fn tampered_ciphertext_fails_closed() {
    let aead = Aes256GcmAead::new(&[9u8; 32]);
    let nonce = [1u8; 12];
    let mut ct = aead.seal(&nonce, b"aad", b"secret payload").unwrap();
    ct[0] ^= 0x01;
    assert!(
        aead.open(&nonce, b"aad", &ct).is_err(),
        "tamper must not open"
    );
}

#[test]
fn wrong_aad_fails_closed() {
    let aead = Aes256GcmAead::new(&[9u8; 32]);
    let nonce = [1u8; 12];
    let ct = aead.seal(&nonce, b"aad-A", b"secret payload").unwrap();
    assert!(
        aead.open(&nonce, b"aad-B", &ct).is_err(),
        "AAD mismatch must not open"
    );
}

#[test]
fn wrong_nonce_fails_closed() {
    let aead = Aes256GcmAead::new(&[9u8; 32]);
    let ct = aead.seal(&[1u8; 12], b"aad", b"secret payload").unwrap();
    assert!(
        aead.open(&[2u8; 12], b"aad", &ct).is_err(),
        "wrong nonce must not open"
    );
}
