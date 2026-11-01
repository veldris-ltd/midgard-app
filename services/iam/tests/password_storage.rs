//! MG-SEC-11 — stored passwords: new ones are Argon2id; a stored hash cannot be
//! weakened below the enforced cost floor (Argon2id or legacy PBKDF2); and a legacy
//! entry is re-hashed with Argon2id and persisted when its owner next signs in.
//! Also pins the primitives underneath to published known answers (MG-SEC-10).
//! [MG-SEC-10] [MG-SEC-11] [RC-29]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::path::PathBuf;

use argon2::{Algorithm, Argon2, AssociatedData, ParamsBuilder, Version};
use iam::{
    check_enrolment_hash, AuthError, Credentials, Directory, DurableDirectory, Enrollment, Idp,
    InMemoryDirectory, MfaProof, PasswordHasher, PasswordUpgrade, Pbkdf2Hasher, TotpSecret,
    UpgradingAuthenticator, UserRecord, MIN_PBKDF2_ITERATIONS,
};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use policy::{ClassificationLevel, Clearance, Purpose, Role};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::hash::pbkdf2_hmac_sha256;
use pqc_agility::Rng;

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let b = z.to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&b[..take]);
            i += take;
        }
    }
}

const NOW: u64 = 1_700_000_000;
const TTL: u64 = 3600;
const PASSWORD: &[u8] = b"correct horse battery staple";

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn purpose() -> Purpose {
    Purpose("investigation".into())
}

fn analyst(password_hash: String, totp: Option<TotpSecret>) -> Enrollment {
    Enrollment {
        user: UserRecord {
            roles: [Role("analyst".into())].into_iter().collect(),
            clearance: Clearance {
                level: ClassificationLevel::SECRET,
                compartments: BTreeSet::new(),
            },
            purposes: [purpose()].into_iter().collect(),
        },
        credentials: Credentials {
            password_hash,
            totp,
            passkeys: vec![],
        },
    }
}

fn sealer() -> Box<KmsSealer<SoftwareTrustRoot>> {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "dir-root", &mut SeedRng(2)).unwrap();
    Box::new(KmsSealer::new(Kms::new(provider, root)))
}

fn scratch(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("iam-mg-sec-11-{name}-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

/// A legacy entry exactly as the pre-Argon2id code wrote it:
/// `pbkdf2-sha256$i=<iterations>$<hex salt>$<hex hash>`.
fn legacy_pbkdf2(password: &[u8], iterations: u32) -> String {
    let salt: Vec<u8> = (0u8..16).collect();
    let mut dk = [0u8; 32];
    pbkdf2_hmac_sha256(password, &salt, iterations, &mut dk);
    format!("pbkdf2-sha256$i={iterations}${}${}", hex(&salt), hex(&dk))
}

// ---- known answers ---------------------------------------------------------

#[test]
fn legacy_pbkdf2_entries_are_standard_pbkdf2_hmac_sha256() {
    // Computed independently (Python `hashlib.pbkdf2_hmac`) for the stored-format
    // salt length and the floor iteration count: a legacy entry written by any
    // correct PBKDF2-HMAC-SHA256 verifies here.
    let salt: Vec<u8> = (0u8..16).collect();
    let expected = "18465cade096c185b5f60765891fca3f74477e23fd9b6fc88faf7831a91753ac";
    let stored = format!(
        "pbkdf2-sha256$i={MIN_PBKDF2_ITERATIONS}${}${expected}",
        hex(&salt)
    );
    assert_eq!(stored, legacy_pbkdf2(PASSWORD, MIN_PBKDF2_ITERATIONS));
    assert!(Pbkdf2Hasher::production().verify(PASSWORD, &stored));
    assert!(!Pbkdf2Hasher::production().verify(b"correct horse battery stapl", &stored));

    // And the published RFC 6070-style SHA-256 vectors, through the same function.
    for (c, dk) in [
        (
            1,
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b",
        ),
        (
            4096,
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a",
        ),
    ] {
        let mut out = [0u8; 32];
        pbkdf2_hmac_sha256(b"password", b"salt", c, &mut out);
        assert_eq!(hex(&out), dk, "c={c}");
    }
}

#[test]
fn the_vendored_argon2id_matches_rfc_9106() {
    // RFC 9106 §5.3 Argon2id test vector.
    let mut builder = ParamsBuilder::new();
    builder
        .m_cost(32)
        .t_cost(3)
        .p_cost(4)
        .output_len(32)
        .data(AssociatedData::new(&[0x04; 12]).unwrap());
    let params = builder.build().unwrap();
    let argon2 =
        Argon2::new_with_secret(&[0x03; 8], Algorithm::Argon2id, Version::V0x13, params).unwrap();
    let mut tag = [0u8; 32];
    argon2
        .hash_password_into(&[0x01; 32], &[0x02; 16], &mut tag)
        .unwrap();
    assert_eq!(
        tag.to_vec(),
        unhex("0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659")
    );
}

// ---- floors ------------------------------------------------------------------

#[test]
fn new_passwords_are_argon2id_and_a_weakened_entry_is_refused() {
    let hasher = Pbkdf2Hasher::production();
    let stored = hasher.hash(PASSWORD);
    assert!(
        stored.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "{stored}"
    );
    assert!(hasher.verify(PASSWORD, &stored));
    assert_eq!(check_enrolment_hash(&stored), Ok(()));

    // Rewrite the cost in the stored string: the right password no longer verifies.
    for (from, to) in [("m=19456", "m=64"), ("t=2", "t=1")] {
        let tampered = stored.replacen(from, to, 1);
        assert!(!hasher.verify(PASSWORD, &tampered), "{tampered}");
        assert!(check_enrolment_hash(&tampered).is_err());
    }

    // A legacy entry below the PBKDF2 floor is refused by the production hasher, and
    // a new enrolment must be Argon2id whatever its PBKDF2 cost.
    let weak = legacy_pbkdf2(PASSWORD, 1_000);
    assert!(!hasher.verify(PASSWORD, &weak));
    assert!(check_enrolment_hash(&weak)
        .unwrap_err()
        .contains("Argon2id"));
}

// ---- rehash on sign-in -------------------------------------------------------

#[test]
fn a_legacy_entry_is_upgraded_to_argon2id_on_sign_in_and_survives_a_restart() {
    let path = scratch("upgrade");
    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();
    let hasher = Pbkdf2Hasher::production();
    let legacy = legacy_pbkdf2(PASSWORD, MIN_PBKDF2_ITERATIONS);
    {
        let mut dir = DurableDirectory::open(&path, sealer()).unwrap();
        dir.enroll("alice", analyst(legacy.clone(), None)).unwrap();

        // A wrong password changes nothing.
        let wrong = UpgradingAuthenticator::new(&idp, &mut dir, &hasher).authenticate(
            "alice",
            b"wrong",
            MfaProof::None,
            purpose(),
            NOW,
            TTL,
        );
        assert_eq!(wrong.unwrap_err(), AuthError::BadPassword);
        assert_eq!(dir.get("alice").unwrap().credentials.password_hash, legacy);

        // The right one signs in and replaces the legacy hash with Argon2id.
        let login = UpgradingAuthenticator::new(&idp, &mut dir, &hasher)
            .authenticate("alice", PASSWORD, MfaProof::None, purpose(), NOW, TTL)
            .expect("legacy credential still signs in");
        assert!(!login.token.is_empty());
        assert_eq!(login.upgrade, PasswordUpgrade::Upgraded);
        let upgraded = dir.get("alice").unwrap().credentials.password_hash.clone();
        assert!(upgraded.starts_with("$argon2id$"), "{upgraded}");
    } // directory closed

    // The upgrade was journalled: it is what a restart replays.
    let mut dir = DurableDirectory::open(&path, sealer()).unwrap();
    let stored = dir.get("alice").unwrap().credentials.password_hash.clone();
    assert!(stored.starts_with("$argon2id$"), "{stored}");
    assert!(!hasher.needs_rehash(&stored));
    let again = UpgradingAuthenticator::new(&idp, &mut dir, &hasher)
        .authenticate("alice", PASSWORD, MfaProof::None, purpose(), NOW, TTL)
        .expect("the upgraded credential signs in");
    assert_eq!(again.upgrade, PasswordUpgrade::Current);
    assert_eq!(dir.get("alice").unwrap().credentials.password_hash, stored);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn nothing_is_upgraded_unless_every_factor_passes() {
    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();
    let hasher = Pbkdf2Hasher::production();
    let legacy = legacy_pbkdf2(PASSWORD, MIN_PBKDF2_ITERATIONS);
    let totp = TotpSecret::new(b"a-shared-totp-secret-32-bytes-xx".to_vec());
    let mut dir = InMemoryDirectory::new();
    dir.enroll("bob", analyst(legacy.clone(), Some(totp.clone())));

    // Right password, missing or wrong second factor: refused, hash untouched.
    for mfa in [MfaProof::None, MfaProof::Totp("000000".into())] {
        let r = UpgradingAuthenticator::new(&idp, &mut dir, &hasher).authenticate(
            "bob",
            PASSWORD,
            mfa,
            purpose(),
            NOW,
            TTL,
        );
        assert!(r.is_err());
        assert_eq!(
            dir.enrollment("bob").unwrap().credentials.password_hash,
            legacy
        );
    }

    let login = UpgradingAuthenticator::new(&idp, &mut dir, &hasher)
        .authenticate(
            "bob",
            PASSWORD,
            MfaProof::Totp(totp.code_at(NOW)),
            purpose(),
            NOW,
            TTL,
        )
        .expect("both factors pass");
    assert_eq!(login.upgrade, PasswordUpgrade::Upgraded);
    assert!(dir
        .enrollment("bob")
        .unwrap()
        .credentials
        .password_hash
        .starts_with("$argon2id$"));
}

#[test]
fn a_legacy_entry_below_the_floor_is_refused_and_left_alone() {
    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();
    let hasher = Pbkdf2Hasher::production();
    let weak = legacy_pbkdf2(PASSWORD, 1_000);
    let mut dir = InMemoryDirectory::new();
    dir.enroll("carol", analyst(weak.clone(), None));

    let r = UpgradingAuthenticator::new(&idp, &mut dir, &hasher).authenticate(
        "carol",
        PASSWORD,
        MfaProof::None,
        purpose(),
        NOW,
        TTL,
    );
    assert_eq!(r.unwrap_err(), AuthError::BadPassword);
    assert_eq!(
        dir.enrollment("carol").unwrap().credentials.password_hash,
        weak
    );
}

#[test]
fn an_upgrade_never_overwrites_a_newer_enrolment() {
    let mut dir = InMemoryDirectory::new();
    dir.enroll("dave", analyst("newer".into(), None));
    assert_eq!(
        dir.replace_password_hash("dave", "older", "upgraded".into()),
        Ok(false)
    );
    assert_eq!(
        dir.enrollment("dave").unwrap().credentials.password_hash,
        "newer"
    );
    assert_eq!(
        dir.replace_password_hash("nobody", "older", "upgraded".into()),
        Ok(false)
    );
}
