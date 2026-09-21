//! SHA-256 known-answer tests (NIST FIPS 180-4 / CAVP). These anchor the one
//! hand-written primitive in the crypto tree; any error fails CI loudly.

use pqc_agility::hash::{sha256, Digest, Sha256};

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
