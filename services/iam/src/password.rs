//! Password verification. A stored credential is a salted, high-iteration
//! **PBKDF2-HMAC-SHA256** hash (RFC 8018 / NIST SP 800-132), encoded as a
//! self-describing PHC-style string and compared in constant time. A fresh 128-bit
//! salt per user defeats rainbow tables; the iteration count sets the work factor.
//!
//! ## Argon2id
//! Argon2id (memory-hard) is the preferred production KDF. Where an Argon2 crate is
//! vendored it drops in behind the [`PasswordHasher`] seam with no change to callers
//! or the stored-hash format (the algorithm tag distinguishes them). PBKDF2-SHA256 —
//! built here on the enclosure's own HMAC-SHA256, no external dependency — is the
//! air-gapped default. [NFR-PRV-1]

use pqc_agility::hash::{ct_eq, pbkdf2_hmac_sha256};
use pqc_agility::{OsRng, Rng};

use crate::enc::{hex, unhex};

const DK_LEN: usize = 32; // one HMAC-SHA256 block
const SALT_LEN: usize = 16;
/// The air-gapped default work factor (OWASP-class for PBKDF2-HMAC-SHA256).
pub const DEFAULT_ITERATIONS: u32 = 210_000;

/// Something that hashes and verifies passwords. Argon2id implements this in a
/// production build; [`Pbkdf2Hasher`] is the offline default.
pub trait PasswordHasher {
    /// Hash `password` with a fresh random salt, returning a PHC-style string.
    fn hash(&self, password: &[u8]) -> String;
    /// Verify `password` against a stored PHC string. Constant-time; fails closed on
    /// a malformed or non-matching hash.
    fn verify(&self, password: &[u8], encoded: &str) -> bool;
    /// Spend the work of verifying `password` against a hash this hasher produced, and
    /// discard the result — so refusing an unknown principal takes as long as refusing
    /// a wrong password, and response time does not reveal who is enrolled. The default
    /// does no work; a hasher with a real work factor overrides it. [NFR-SEC-2]
    fn spend_verification(&self, _password: &[u8]) {}
}

/// PBKDF2-HMAC-SHA256 password hasher with a configurable iteration count.
pub struct Pbkdf2Hasher {
    iterations: u32,
}

impl Pbkdf2Hasher {
    /// A hasher with an explicit iteration count.
    pub fn new(iterations: u32) -> Self {
        Self {
            iterations: iterations.max(1),
        }
    }

    /// The production preset ([`DEFAULT_ITERATIONS`]).
    pub fn production() -> Self {
        Self::new(DEFAULT_ITERATIONS)
    }

    fn hash_with_salt(&self, password: &[u8], salt: &[u8]) -> [u8; DK_LEN] {
        pbkdf2_hmac_sha256_32(password, salt, self.iterations)
    }
}

impl Default for Pbkdf2Hasher {
    fn default() -> Self {
        Self::production()
    }
}

impl PasswordHasher for Pbkdf2Hasher {
    fn hash(&self, password: &[u8]) -> String {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill(&mut salt);
        let dk = self.hash_with_salt(password, &salt);
        // PHC-style, hex fields: pbkdf2-sha256$i=<iters>$<salt>$<hash>
        format!(
            "pbkdf2-sha256$i={}${}${}",
            self.iterations,
            hex(&salt),
            hex(&dk)
        )
    }

    fn verify(&self, password: &[u8], encoded: &str) -> bool {
        match parse_phc(encoded) {
            Some((iters, salt, expected)) => {
                let dk = pbkdf2_hmac_sha256_32(password, &salt, iters);
                ct_eq(&dk, &expected)
            }
            None => false,
        }
    }

    fn spend_verification(&self, password: &[u8]) {
        let _ = self.hash_with_salt(password, &[0u8; SALT_LEN]);
    }
}

/// Parse `pbkdf2-sha256$i=<iters>$<hexsalt>$<hexhash>`.
fn parse_phc(s: &str) -> Option<(u32, Vec<u8>, Vec<u8>)> {
    let mut parts = s.split('$');
    if parts.next()? != "pbkdf2-sha256" {
        return None;
    }
    let iters = parts.next()?.strip_prefix("i=")?.parse::<u32>().ok()?;
    let salt = unhex(parts.next()?)?;
    let hash = unhex(parts.next()?)?;
    if parts.next().is_some() {
        return None; // trailing junk
    }
    Some((iters, salt, hash))
}

/// PBKDF2 with HMAC-SHA256 as the PRF (RFC 8018 §5.2), derived-key length fixed at
/// one hash block (32 bytes). Computed by the vetted RustCrypto `pbkdf2` crate via
/// `pqc-agility`, not a local loop. [MG-SEC-10]
fn pbkdf2_hmac_sha256_32(password: &[u8], salt: &[u8], iterations: u32) -> [u8; DK_LEN] {
    let mut out = [0u8; DK_LEN];
    pbkdf2_hmac_sha256(password, salt, iterations, &mut out);
    out
}
