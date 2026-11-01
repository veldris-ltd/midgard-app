//! Password storage and verification. [NFR-SEC-2] [MG-SEC-11]
//!
//! ## Stored format
//! New passwords are stored as **Argon2id** (RFC 9106, memory-hard) in the standard
//! PHC string format, `$argon2id$v=19$m=<KiB>,t=<passes>,p=<lanes>$<salt>$<hash>`,
//! with a fresh 128-bit salt from the OS RNG and a 256-bit output, computed by the
//! vetted, vendored RustCrypto `argon2` crate. Entries written before Argon2id —
//! `pbkdf2-sha256$i=<iterations>$<hex salt>$<hex hash>`, PBKDF2-HMAC-SHA256 (RFC 8018)
//! — still verify, so nobody is locked out by the change of format, and are replaced
//! with Argon2id the next time their owner signs in (below).
//!
//! ## Enforced cost floors
//! A stored hash carries its own cost parameters, so without a floor anyone able to
//! alter a directory entry could weaken it (say to one PBKDF2 iteration, or Argon2id
//! with 8 KiB) and make it cheap to crack. Verification therefore **refuses** — fails
//! closed, exactly as for a wrong password — any entry below:
//!
//! - Argon2id: m ≥ [`ARGON2_MIN_MEMORY_KIB`] (19 MiB), t ≥ [`ARGON2_MIN_TIME_COST`] (2),
//!   p ≥ [`ARGON2_MIN_PARALLELISM`] (1) — OWASP's minimum Argon2id configuration;
//!   only Argon2id (not Argon2i/Argon2d), version 19, a ≥128-bit salt and a 256-bit
//!   output are accepted;
//! - PBKDF2-HMAC-SHA256: [`MIN_PBKDF2_ITERATIONS`] (210 000, the cost every legacy
//!   entry was written at), with a ≥128-bit salt and a 256-bit output. A higher
//!   floor would lock out every existing account before it could be upgraded; no
//!   new entry is ever PBKDF2, and each legacy one becomes Argon2id at its owner's
//!   next sign-in.
//!
//! It also refuses entries *above* a ceiling ([`ARGON2_MAX_MEMORY_KIB`],
//! [`ARGON2_MAX_TIME_COST`], [`ARGON2_MAX_PARALLELISM`], [`MAX_PBKDF2_ITERATIONS`]),
//! so a tampered entry cannot turn one sign-in into a memory or CPU exhaustion.
//! [`check_enrolment_hash`] applies the same floors to a hash handed in for a new
//! enrolment, and requires it to be Argon2id.
//!
//! ## Rehash on sign-in
//! [`PasswordHasher::needs_rehash`] flags a hash that verified but is not in the
//! current format (any legacy PBKDF2 entry, or Argon2id weaker than the current
//! parameters). [`crate::UpgradingAuthenticator`] uses it: after a sign-in passes every
//! check it re-hashes the password just presented with Argon2id and persists that to
//! the directory, so legacy entries disappear as people sign in.
//!
//! ## Test cost knob
//! [`Argon2idHasher::new`] (also reachable as `Pbkdf2Hasher::new`) builds a hasher
//! that writes *legacy* PBKDF2 at a chosen iteration count and accepts that count,
//! which keeps tests that only need *a* password fast. It is a knob for debug builds
//! only: in a release build (`debug_assertions` off, which is every production
//! artefact) the count is raised to [`MIN_PBKDF2_ITERATIONS`], so it cannot weaken a
//! production build — the same convention as `pqc-agility`'s `test-backend` guard.
//! Argon2id has no knob at all: its floor holds in every build.

use argon2::password_hash::{PasswordHash, PasswordHasher as _, PasswordVerifier as _, SaltString};
use argon2::{Algorithm, Argon2, Params, Version, ARGON2ID_IDENT};
use pqc_agility::hash::{ct_eq, pbkdf2_hmac_sha256};
use pqc_agility::{OsRng, Rng};

use crate::enc::{hex, unhex};

/// Derived-key / Argon2 output length: 256 bits.
const DK_LEN: usize = 32;
/// Salt length for new hashes, and the shortest salt a stored hash may carry: 128 bits.
const SALT_LEN: usize = 16;
/// The legacy PBKDF2 algorithm tag.
const PBKDF2_TAG: &str = "pbkdf2-sha256";

/// The fewest PBKDF2-HMAC-SHA256 iterations a stored legacy hash may claim and still
/// verify: the cost every legacy entry was written at, so existing accounts can sign
/// in once more and be upgraded to Argon2id, while an entry weakened below it is
/// refused. [MG-SEC-11]
pub const MIN_PBKDF2_ITERATIONS: u32 = 210_000;
/// The most PBKDF2 iterations a stored hash may claim (a tampered entry must not turn
/// one sign-in into minutes of CPU). [MG-SEC-11]
pub const MAX_PBKDF2_ITERATIONS: u32 = 10_000_000;
/// The PBKDF2 work factor. Kept under its old name for existing callers; it is now
/// the enforced floor, [`MIN_PBKDF2_ITERATIONS`]. New passwords are Argon2id.
pub const DEFAULT_ITERATIONS: u32 = MIN_PBKDF2_ITERATIONS;

/// Argon2id memory floor in KiB (19 MiB, OWASP's minimum). [MG-SEC-11]
pub const ARGON2_MIN_MEMORY_KIB: u32 = 19 * 1024;
/// Argon2id time-cost (passes) floor. [MG-SEC-11]
pub const ARGON2_MIN_TIME_COST: u32 = 2;
/// Argon2id parallelism (lanes) floor. [MG-SEC-11]
pub const ARGON2_MIN_PARALLELISM: u32 = 1;
/// Argon2id memory ceiling in KiB (256 MiB). [MG-SEC-11]
pub const ARGON2_MAX_MEMORY_KIB: u32 = 256 * 1024;
/// Argon2id time-cost ceiling. [MG-SEC-11]
pub const ARGON2_MAX_TIME_COST: u32 = 16;
/// Argon2id parallelism ceiling. [MG-SEC-11]
pub const ARGON2_MAX_PARALLELISM: u32 = 16;

/// The parameters new Argon2id hashes are written with: currently the floor itself.
/// Raising these later makes [`PasswordHasher::needs_rehash`] upgrade older entries
/// on sign-in. [MG-SEC-11]
pub const ARGON2_MEMORY_KIB: u32 = ARGON2_MIN_MEMORY_KIB;
/// See [`ARGON2_MEMORY_KIB`].
pub const ARGON2_TIME_COST: u32 = ARGON2_MIN_TIME_COST;
/// See [`ARGON2_MEMORY_KIB`].
pub const ARGON2_PARALLELISM: u32 = ARGON2_MIN_PARALLELISM;

/// Something that hashes and verifies passwords. [`Argon2idHasher`] is the platform's.
pub trait PasswordHasher {
    /// Hash `password` with a fresh random salt, returning a PHC-style string.
    fn hash(&self, password: &[u8]) -> String;
    /// Verify `password` against a stored PHC string. Constant-time; fails closed on
    /// a malformed or non-matching hash, and on one whose cost is outside the
    /// enforced bounds. [MG-SEC-11]
    fn verify(&self, password: &[u8], encoded: &str) -> bool;
    /// Spend the work of verifying `password` against a hash this hasher produced, and
    /// discard the result — so refusing an unknown principal takes as long as refusing
    /// a wrong password, and response time does not reveal who is enrolled. The default
    /// does no work; a hasher with a real work factor overrides it. [NFR-SEC-2]
    fn spend_verification(&self, _password: &[u8]) {}
    /// Whether `encoded`, a hash that has just verified, should be replaced by
    /// [`rehash`](Self::rehash) of the same password: it is in a legacy format or
    /// weaker than the current parameters. The default never asks. [MG-SEC-11]
    fn needs_rehash(&self, _encoded: &str) -> bool {
        false
    }
    /// Hash `password` in the current preferred format, to replace a hash that
    /// [`needs_rehash`](Self::needs_rehash) flagged. Defaults to
    /// [`hash`](Self::hash). [MG-SEC-11]
    fn rehash(&self, password: &[u8]) -> String {
        self.hash(password)
    }
}

/// The platform password hasher: writes Argon2id, verifies Argon2id and legacy
/// PBKDF2-HMAC-SHA256 within the enforced cost bounds, and flags legacy or weaker
/// entries for rehash on sign-in. See the module docs. [MG-SEC-11]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Argon2idHasher {
    /// `Some(iterations)` for the debug-only legacy writer [`Argon2idHasher::new`];
    /// `None` writes Argon2id.
    legacy_pbkdf2: Option<u32>,
    /// The fewest PBKDF2 iterations a stored legacy hash may claim and verify.
    pbkdf2_floor: u32,
}

/// The hasher's former name, kept so existing callers compile unchanged. It is the
/// same type: `Pbkdf2Hasher::production()` now stores **Argon2id**. [MG-SEC-11]
pub type Pbkdf2Hasher = Argon2idHasher;

impl Argon2idHasher {
    /// The production hasher: writes Argon2id at [`ARGON2_MEMORY_KIB`] /
    /// [`ARGON2_TIME_COST`] / [`ARGON2_PARALLELISM`]; accepts legacy PBKDF2 only at
    /// [`MIN_PBKDF2_ITERATIONS`] or more. [MG-SEC-11]
    pub fn production() -> Self {
        Self {
            legacy_pbkdf2: None,
            pbkdf2_floor: MIN_PBKDF2_ITERATIONS,
        }
    }

    /// A hasher that writes **legacy** PBKDF2-HMAC-SHA256 at `iterations`, and accepts
    /// legacy hashes down to that count — the test and development cost knob. In a
    /// release build `iterations` is raised to [`MIN_PBKDF2_ITERATIONS`], so the knob
    /// cannot weaken production. Argon2id entries are held to their floor regardless.
    /// [MG-SEC-11]
    pub fn new(iterations: u32) -> Self {
        let iterations = if cfg!(debug_assertions) {
            iterations.max(1)
        } else {
            iterations.max(MIN_PBKDF2_ITERATIONS)
        };
        Self {
            legacy_pbkdf2: Some(iterations),
            pbkdf2_floor: iterations.min(MIN_PBKDF2_ITERATIONS),
        }
    }

    fn hash_argon2id(password: &[u8]) -> String {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill(&mut salt);
        let salt = SaltString::encode_b64(&salt).expect("a 16-byte salt encodes");
        argon2id()
            .hash_password(password, &salt)
            .expect("Argon2id with the constant parameters cannot fail")
            .to_string()
    }

    fn hash_pbkdf2(password: &[u8], iterations: u32) -> String {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill(&mut salt);
        let dk = pbkdf2_32(password, &salt, iterations);
        format!("{PBKDF2_TAG}$i={iterations}${}${}", hex(&salt), hex(&dk))
    }
}

impl Default for Argon2idHasher {
    fn default() -> Self {
        Self::production()
    }
}

impl PasswordHasher for Argon2idHasher {
    fn hash(&self, password: &[u8]) -> String {
        match self.legacy_pbkdf2 {
            Some(iterations) => Self::hash_pbkdf2(password, iterations),
            None => Self::hash_argon2id(password),
        }
    }

    fn verify(&self, password: &[u8], encoded: &str) -> bool {
        let Ok(stored) = Stored::parse(encoded) else {
            return false;
        };
        if stored.check_bounds(self.pbkdf2_floor).is_err() {
            return false;
        }
        stored.verify(password)
    }

    fn spend_verification(&self, password: &[u8]) {
        match self.legacy_pbkdf2 {
            Some(iterations) => {
                let _ = pbkdf2_32(password, &[0u8; SALT_LEN], iterations);
            }
            None => {
                let mut out = [0u8; DK_LEN];
                let _ = argon2id().hash_password_into(password, &[0u8; SALT_LEN], &mut out);
            }
        }
    }

    fn needs_rehash(&self, encoded: &str) -> bool {
        match Stored::parse(encoded) {
            Ok(Stored::Pbkdf2 { .. }) => true,
            Ok(Stored::Argon2id { params, .. }) => {
                params.m_cost() < ARGON2_MEMORY_KIB
                    || params.t_cost() < ARGON2_TIME_COST
                    || params.p_cost() < ARGON2_PARALLELISM
            }
            Err(_) => false,
        }
    }

    fn rehash(&self, password: &[u8]) -> String {
        Self::hash_argon2id(password)
    }
}

/// Check a password hash handed in for a **new** enrolment: it must be Argon2id
/// within the enforced cost bounds. Legacy PBKDF2 is refused here — it is only
/// accepted from entries that already exist. `Err` says why, as a phrase that reads
/// after the field name ("is …", "has …"). [MG-SEC-11]
pub fn check_enrolment_hash(encoded: &str) -> Result<(), String> {
    match Stored::parse(encoded)? {
        Stored::Pbkdf2 { .. } => Err(format!(
            "is a legacy {PBKDF2_TAG} hash; new passwords must be stored as Argon2id"
        )),
        argon2id => argon2id.check_bounds(MIN_PBKDF2_ITERATIONS),
    }
}

fn argon2id() -> Argon2<'static> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_TIME_COST,
        ARGON2_PARALLELISM,
        Some(DK_LEN),
    )
    .expect("the constant Argon2id parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

fn pbkdf2_32(password: &[u8], salt: &[u8], iterations: u32) -> [u8; DK_LEN] {
    let mut out = [0u8; DK_LEN];
    pbkdf2_hmac_sha256(password, salt, iterations, &mut out);
    out
}

/// A parsed stored hash.
enum Stored<'a> {
    Argon2id {
        phc: Box<PasswordHash<'a>>,
        params: Params,
    },
    Pbkdf2 {
        iterations: u32,
        salt: Vec<u8>,
        hash: Vec<u8>,
    },
}

impl<'a> Stored<'a> {
    fn parse(encoded: &'a str) -> Result<Self, String> {
        if encoded.starts_with('$') {
            Self::parse_argon2id(encoded)
        } else if encoded.starts_with(PBKDF2_TAG) {
            Self::parse_pbkdf2(encoded)
                .ok_or_else(|| format!("is not a well-formed {PBKDF2_TAG} hash"))
        } else {
            Err(format!(
                "is neither an Argon2id PHC string nor a {PBKDF2_TAG} hash"
            ))
        }
    }

    fn parse_argon2id(encoded: &'a str) -> Result<Self, String> {
        let phc = PasswordHash::new(encoded)
            .map_err(|e| format!("is not a well-formed PHC string ({e})"))?;
        if phc.algorithm != ARGON2ID_IDENT {
            return Err(format!("uses {}, not argon2id", phc.algorithm));
        }
        if phc.version != Some(Version::V0x13.into()) {
            return Err("is not Argon2 version 19".into());
        }
        let params =
            Params::try_from(&phc).map_err(|e| format!("has invalid Argon2 parameters ({e})"))?;
        let mut buf = [0u8; 64];
        let salt_len = phc
            .salt
            .ok_or("has no salt")?
            .decode_b64(&mut buf)
            .map_err(|e| format!("has an invalid salt ({e})"))?
            .len();
        if salt_len < SALT_LEN {
            return Err(format!(
                "has a {salt_len}-byte salt; at least {SALT_LEN} are required"
            ));
        }
        if phc.hash.as_ref().map(|h| h.len()) != Some(DK_LEN) {
            return Err(format!("does not carry a {DK_LEN}-byte hash"));
        }
        Ok(Stored::Argon2id {
            phc: Box::new(phc),
            params,
        })
    }

    /// `pbkdf2-sha256$i=<iterations>$<hex salt>$<hex hash>`.
    fn parse_pbkdf2(encoded: &str) -> Option<Self> {
        let mut parts = encoded.split('$');
        if parts.next()? != PBKDF2_TAG {
            return None;
        }
        let iterations = parts.next()?.strip_prefix("i=")?.parse::<u32>().ok()?;
        let salt = unhex(parts.next()?)?;
        let hash = unhex(parts.next()?)?;
        if parts.next().is_some() {
            return None; // trailing junk
        }
        Some(Stored::Pbkdf2 {
            iterations,
            salt,
            hash,
        })
    }

    /// The cost bounds a stored hash must sit within to be verified at all.
    fn check_bounds(&self, pbkdf2_floor: u32) -> Result<(), String> {
        match self {
            Stored::Argon2id { params, .. } => {
                let (m, t, p) = (params.m_cost(), params.t_cost(), params.p_cost());
                if m < ARGON2_MIN_MEMORY_KIB
                    || t < ARGON2_MIN_TIME_COST
                    || p < ARGON2_MIN_PARALLELISM
                {
                    return Err(format!(
                        "has Argon2id cost m={m},t={t},p={p}, below the floor \
                         m={ARGON2_MIN_MEMORY_KIB},t={ARGON2_MIN_TIME_COST},p={ARGON2_MIN_PARALLELISM}"
                    ));
                }
                if m > ARGON2_MAX_MEMORY_KIB
                    || t > ARGON2_MAX_TIME_COST
                    || p > ARGON2_MAX_PARALLELISM
                {
                    return Err(format!(
                        "has Argon2id cost m={m},t={t},p={p}, above the ceiling \
                         m={ARGON2_MAX_MEMORY_KIB},t={ARGON2_MAX_TIME_COST},p={ARGON2_MAX_PARALLELISM}"
                    ));
                }
                Ok(())
            }
            Stored::Pbkdf2 {
                iterations,
                salt,
                hash,
            } => {
                if *iterations < pbkdf2_floor {
                    return Err(format!(
                        "uses {iterations} PBKDF2 iterations; at least {pbkdf2_floor} are required"
                    ));
                }
                if *iterations > MAX_PBKDF2_ITERATIONS {
                    return Err(format!(
                        "uses {iterations} PBKDF2 iterations; at most {MAX_PBKDF2_ITERATIONS} are accepted"
                    ));
                }
                if salt.len() < SALT_LEN {
                    return Err(format!(
                        "has a {}-byte salt; at least {SALT_LEN} are required",
                        salt.len()
                    ));
                }
                if hash.len() != DK_LEN {
                    return Err(format!("does not carry a {DK_LEN}-byte hash"));
                }
                Ok(())
            }
        }
    }

    /// Verify `password` (call only after [`Self::check_bounds`]). Constant-time.
    fn verify(&self, password: &[u8]) -> bool {
        match self {
            // `verify_password` recomputes with the entry's own (bounds-checked)
            // parameters and compares the outputs in constant time.
            Stored::Argon2id { phc, .. } => argon2id().verify_password(password, phc).is_ok(),
            Stored::Pbkdf2 {
                iterations,
                salt,
                hash,
            } => ct_eq(&pbkdf2_32(password, salt, *iterations), hash),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An Argon2id PHC string for `password` with explicit parameters, as a tampered
    /// or foreign entry would carry them.
    fn argon2id_with(password: &[u8], m: u32, t: u32, p: u32) -> String {
        let params = Params::new(m, t, p, Some(DK_LEN)).unwrap();
        let salt = SaltString::encode_b64(&[7u8; SALT_LEN]).unwrap();
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password(password, &salt)
            .unwrap()
            .to_string()
    }

    #[test]
    fn production_writes_argon2id_at_the_floor_and_verifies_it() {
        let h = Argon2idHasher::production();
        let enc = h.hash(b"correct horse");
        assert!(enc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{enc}");
        assert!(h.verify(b"correct horse", &enc));
        assert!(!h.verify(b"correct horsf", &enc));
        assert!(!h.needs_rehash(&enc));
        assert_eq!(check_enrolment_hash(&enc), Ok(()));
    }

    #[test]
    fn a_weakened_argon2id_entry_is_refused_even_with_the_right_password() {
        let h = Argon2idHasher::production();
        for (m, t, p) in [(8, 1, 1), (19 * 1024, 1, 1), (4096, 2, 1)] {
            let weak = argon2id_with(b"pw", m, t, p);
            assert!(!h.verify(b"pw", &weak), "m={m},t={t},p={p} must be refused");
            assert!(check_enrolment_hash(&weak).is_err());
        }
    }

    #[test]
    fn argon2_variants_other_than_argon2id_are_refused() {
        let enc = Argon2idHasher::production().hash(b"pw");
        let argon2i = enc.replacen("$argon2id$", "$argon2i$", 1);
        assert!(!Argon2idHasher::production().verify(b"pw", &argon2i));
        assert!(check_enrolment_hash(&argon2i).is_err());
    }

    #[test]
    fn an_argon2id_entry_above_the_ceiling_is_refused_without_running_it() {
        // 4 GiB would be allocated if this were verified; it must be refused first.
        let enc = Argon2idHasher::production().hash(b"pw");
        let huge = enc.replacen("m=19456", "m=4194304", 1);
        assert!(!Argon2idHasher::production().verify(b"pw", &huge));
        assert!(check_enrolment_hash(&huge)
            .unwrap_err()
            .contains("above the ceiling"));
    }

    #[test]
    fn legacy_pbkdf2_below_the_floor_is_refused_by_the_production_hasher() {
        let weak = Argon2idHasher::hash_pbkdf2(b"pw", 1_000);
        assert!(!Argon2idHasher::production().verify(b"pw", &weak));
        let at_floor = Argon2idHasher::hash_pbkdf2(b"pw", MIN_PBKDF2_ITERATIONS);
        assert!(Argon2idHasher::production().verify(b"pw", &at_floor));
        assert!(!Argon2idHasher::production().verify(b"pX", &at_floor));
        // Verified but legacy: it is flagged for an Argon2id rehash…
        assert!(Argon2idHasher::production().needs_rehash(&at_floor));
        // …and refused as the hash for a new enrolment.
        assert!(check_enrolment_hash(&at_floor)
            .unwrap_err()
            .contains("Argon2id"));
    }

    #[test]
    fn rehash_is_always_argon2id() {
        let legacy_writer = Argon2idHasher::new(1_000);
        assert!(legacy_writer.hash(b"pw").starts_with("pbkdf2-sha256$"));
        let upgraded = legacy_writer.rehash(b"pw");
        assert!(upgraded.starts_with("$argon2id$"));
        assert!(Argon2idHasher::production().verify(b"pw", &upgraded));
    }

    #[test]
    fn malformed_entries_fail_closed() {
        let h = Argon2idHasher::production();
        for bad in [
            "",
            "not-a-hash",
            "$argon2id$",
            "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$",
            "pbkdf2-sha256$i=600000$00$00",
            "pbkdf2-sha256$i=600000$zz$zz",
        ] {
            assert!(!h.verify(b"pw", bad), "{bad:?}");
            assert!(check_enrolment_hash(bad).is_err(), "{bad:?}");
        }
    }
}
