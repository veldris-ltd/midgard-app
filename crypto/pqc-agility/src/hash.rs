//! SHA-256 (FIPS 180-4), HMAC-SHA256 (RFC 2104), HKDF-SHA256 (RFC 5869),
//! PBKDF2-HMAC-SHA256 (RFC 8018) and a constant-time compare. [MG-SEC-10]
//!
//! None of these is hand-written: each is a thin wrapper over the vetted, vendored
//! RustCrypto crates (`sha2`, `hmac`, `hkdf`, `pbkdf2`, `subtle`), kept behind the same
//! function signatures the whole workspace already calls. A bug in a home-rolled
//! primitive would pass every round-trip test (both ends share it), so the wrappers
//! are pinned to published known answers instead — NIST SHA-256, RFC 4231 (HMAC),
//! RFC 5869 (HKDF) and the RFC 6070-style PBKDF2-HMAC-SHA256 vectors — in
//! `tests/hash_kat.rs`. Every other crate reuses this module (the [`Digest`] trait and
//! these functions) rather than re-implementing a hash, MAC or KDF.

use hmac::Mac;
use subtle::ConstantTimeEq;

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

/// A streaming digest producing a 32-byte output.
pub trait Digest {
    /// Create a fresh hasher.
    fn new() -> Self
    where
        Self: Sized;
    /// Absorb more input.
    fn update(&mut self, data: &[u8]);
    /// Consume the hasher and produce the digest.
    fn finalize(self) -> [u8; 32];
}

/// SHA-256 hasher. `Clone` supports snapshotting a running transcript hash.
/// Backed by the RustCrypto `sha2` crate. [MG-SEC-10]
#[derive(Clone)]
pub struct Sha256(sha2::Sha256);

impl Digest for Sha256 {
    fn new() -> Self {
        Self(<sha2::Sha256 as sha2::Digest>::new())
    }

    fn update(&mut self, data: &[u8]) {
        sha2::Digest::update(&mut self.0, data);
    }

    fn finalize(self) -> [u8; 32] {
        sha2::Digest::finalize(self.0).into()
    }
}

/// One-shot SHA-256.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize()
}

/// HMAC-SHA256 (RFC 2104), via the RustCrypto `hmac` crate. A key of any length is
/// accepted; one longer than the block is hashed first, per the RFC. [MG-SEC-10]
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC accepts a key of any length");
    mac.update(msg);
    mac.finalize().into_bytes().into()
}

/// HKDF-SHA256 (RFC 5869) producing a single 32-byte output block, via the
/// RustCrypto `hkdf` crate. An empty `salt` is the RFC's "not provided" (a block of
/// zeros), so outputs are unchanged from before the swap. [MG-SEC-10]
pub fn hkdf_sha256_32(salt: &[u8], ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(salt), ikm);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm)
        .expect("32 bytes is within HKDF-SHA256's 255-block output limit");
    okm
}

/// PBKDF2 with HMAC-SHA256 as the PRF (RFC 8018 §5.2), filling `out`, via the
/// RustCrypto `pbkdf2` crate. An `iterations` of 0 is computed as 1 (PBKDF2 is not
/// defined for 0). This only computes: a caller storing passwords enforces its own
/// minimum work factor. [MG-SEC-10]
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password, salt, iterations.max(1), out);
}

/// Constant-time equality over two byte slices, via the RustCrypto `subtle` crate.
/// Slices of different lengths are never equal (the length itself is not secret).
/// [MG-SEC-10]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    bool::from(a.ct_eq(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    #[test]
    fn sha256_empty() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn ct_eq_compares_content_and_length() {
        assert!(ct_eq(b"", b""));
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        // A prefix is not equal, in either order.
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(!ct_eq(b"ab", b"abc"));
        assert!(!ct_eq(b"", b"\0"));
    }
}
