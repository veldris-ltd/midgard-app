//! RC-29 — Given a user, when they authenticate, then credentials + MFA are verified
//! before a PQ-signed token is minted; the directory survives restart; federation
//! works when configured. [RC-29]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;

use iam::{
    federated_login, verify_token, Activation, AuthError, Authenticator, Credentials,
    DurableDirectory, Enrollment, Idp, InMemoryDirectory, MfaProof, PasswordHasher, Pbkdf2Hasher,
    PqFederation, TotpSecret, UserRecord,
};
use policy::{ClassificationLevel, Clearance, Purpose, Role};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, SigId};

use kms::{Kms, KmsSealer, SoftwareTrustRoot};

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

fn analyst_user() -> UserRecord {
    UserRecord {
        roles: [Role("analyst".into())].into_iter().collect(),
        clearance: Clearance {
            level: ClassificationLevel::SECRET,
            compartments: BTreeSet::new(),
        },
        purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

/// A KMS sealer over a software trust root (the directory's at-rest encryption).
fn sealer(seed: u64) -> Box<KmsSealer<SoftwareTrustRoot>> {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "dir-root", &mut SeedRng(seed)).unwrap();
    Box::new(KmsSealer::new(Kms::new(provider, root)))
}

#[test]
fn credentials_and_mfa_are_verified_before_a_token_is_minted() {
    let dir_path = std::env::temp_dir().join(format!("iam-rc29-a-{}.log", NOW));
    let _ = std::fs::remove_file(&dir_path);

    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();
    let hasher = Pbkdf2Hasher::new(1000); // low work factor for a fast test
    let totp = TotpSecret::new(b"a-shared-totp-secret-32-bytes-xx".to_vec());

    let mut dir = DurableDirectory::open(&dir_path, sealer(2)).unwrap();
    dir.enroll(
        "alice",
        Enrollment {
            user: analyst_user(),
            credentials: Credentials {
                password_hash: hasher.hash(b"correct horse battery staple"),
                totp: Some(totp.clone()),
                passkeys: vec![],
                activation: None,
            },
        },
    )
    .unwrap();

    let auth = Authenticator::new(&idp, &dir, &hasher);
    let good_code = totp.code_at(NOW);

    // Wrong password → rejected, no token.
    assert_eq!(
        auth.authenticate(
            "alice",
            b"wrong",
            MfaProof::Totp(good_code.clone()),
            Purpose("investigation".into()),
            NOW,
            TTL
        ),
        Err(AuthError::BadPassword)
    );
    // Right password, missing MFA → rejected.
    assert_eq!(
        auth.authenticate(
            "alice",
            b"correct horse battery staple",
            MfaProof::None,
            Purpose("investigation".into()),
            NOW,
            TTL
        ),
        Err(AuthError::MfaRequired)
    );
    // Right password, wrong TOTP → rejected.
    assert_eq!(
        auth.authenticate(
            "alice",
            b"correct horse battery staple",
            MfaProof::Totp("000000".into()),
            Purpose("investigation".into()),
            NOW,
            TTL
        ),
        Err(AuthError::BadMfa)
    );
    // Unknown subject → rejected.
    assert_eq!(
        auth.authenticate(
            "mallory",
            b"x",
            MfaProof::None,
            Purpose("investigation".into()),
            NOW,
            TTL
        ),
        Err(AuthError::UnknownSubject)
    );

    // Correct password + correct TOTP → a PQ-signed token is minted and verifies.
    let token = auth
        .authenticate(
            "alice",
            b"correct horse battery staple",
            MfaProof::Totp(good_code),
            Purpose("investigation".into()),
            NOW,
            TTL,
        )
        .expect("full authentication succeeds");
    let claims = verify_token(idp.provider(), idp.verifying_key(), &token, NOW + 10).unwrap();
    assert_eq!(claims.subject, "alice");
    assert!(claims.roles.contains(&Role("analyst".into())));
    assert_eq!(claims.level, ClassificationLevel::SECRET);

    let _ = std::fs::remove_file(&dir_path);
}

#[test]
fn the_directory_survives_a_restart() {
    let dir_path = std::env::temp_dir().join(format!("iam-rc29-b-{}.log", NOW));
    let _ = std::fs::remove_file(&dir_path);

    let hasher = Pbkdf2Hasher::new(1000);
    let hash = hasher.hash(b"pw");
    {
        let mut dir = DurableDirectory::open(&dir_path, sealer(2)).unwrap();
        dir.enroll(
            "bob",
            Enrollment {
                user: analyst_user(),
                credentials: Credentials {
                    password_hash: hash.clone(),
                    totp: None,
                    passkeys: vec![],
                    activation: None,
                },
            },
        )
        .unwrap();
        assert_eq!(dir.len(), 1);
    }

    // Reopen with a fresh sealer over the SAME deterministic root → replay.
    let dir2 = DurableDirectory::open(&dir_path, sealer(2)).unwrap();
    let entry = dir2.get("bob").expect("enrollment survived restart");
    assert_eq!(entry.user, analyst_user());
    assert!(hasher.verify(b"pw", &entry.credentials.password_hash));

    // The recovered password still authenticates (no MFA enrolled → no factor needed).
    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();
    let auth = Authenticator::new(&idp, &dir2, &hasher);
    let token = auth
        .authenticate(
            "bob",
            b"pw",
            MfaProof::None,
            Purpose("investigation".into()),
            NOW,
            TTL,
        )
        .expect("recovered credential authenticates");
    assert!(verify_token(idp.provider(), idp.verifying_key(), &token, NOW).is_ok());

    let _ = std::fs::remove_file(&dir_path);
}

#[test]
fn federation_mints_a_local_token_from_a_verified_upstream_assertion() {
    // A trusted upstream IdP signs assertions with its own ML-DSA key.
    let provider = fips_provider();
    let (up_vk, up_sk) = provider
        .sig_for(SigId::MlDsa65)
        .unwrap()
        .keypair(&mut SeedRng(50))
        .unwrap();

    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();
    let mut federation = PqFederation::new(
        fips_provider(),
        up_vk.clone(),
        SigId::MlDsa65,
        "upstream-corp",
        "midgard-idp",
    );

    // The upstream vouches for a federated analyst, scoped to this bridge and fresh.
    let claims = iam::FederatedClaims {
        issuer: "upstream-corp".into(),
        audience: "midgard-idp".into(),
        issued_at: NOW,
        expires_at: NOW + 120,
        nonce: [7u8; 16],
        subject: "carol@partner.example".into(),
        user: analyst_user(),
    };
    let payload = PqFederation::assertion_payload(&claims);
    let sig = provider
        .sig_for(SigId::MlDsa65)
        .unwrap()
        .sign(&up_sk, &payload)
        .unwrap();
    let assertion = PqFederation::encode_assertion(&claims, &sig);

    // Federated login mints a local PQ token.
    let token = federated_login(
        &idp,
        &mut federation,
        &assertion,
        Purpose("investigation".into()),
        NOW,
        TTL,
    )
    .expect("federated login succeeds");
    let out = verify_token(idp.provider(), idp.verifying_key(), &token, NOW).unwrap();
    assert_eq!(out.subject, "carol@partner.example");
    assert!(out.roles.contains(&Role("analyst".into())));

    // A tampered assertion (flip a payload byte) fails closed.
    let mut bad = assertion.clone();
    bad[8] ^= 0xFF;
    assert!(federated_login(
        &idp,
        &mut federation,
        &bad,
        Purpose("investigation".into()),
        NOW,
        TTL
    )
    .is_err());

    // An assertion signed by an untrusted key fails closed.
    let (_other_vk, other_sk) = provider
        .sig_for(SigId::MlDsa65)
        .unwrap()
        .keypair(&mut SeedRng(99))
        .unwrap();
    let sig2 = provider
        .sig_for(SigId::MlDsa65)
        .unwrap()
        .sign(&other_sk, &payload)
        .unwrap();
    let assertion2 = PqFederation::encode_assertion(&claims, &sig2);
    assert!(federated_login(
        &idp,
        &mut federation,
        &assertion2,
        Purpose("investigation".into()),
        NOW,
        TTL
    )
    .is_err());
}

/// The analyst enrollment the activation tests use: approved under dual control, so
/// it has a TOTP secret but no password of its own yet and a pending activation.
fn unactivated(hasher: &Pbkdf2Hasher) -> Enrollment {
    Enrollment {
        user: analyst_user(),
        credentials: Credentials {
            password_hash: String::new(),
            totp: Some(TotpSecret::new(vec![7u8; 20])),
            passkeys: vec![],
            activation: Some(Activation {
                code_hash: hasher.hash(b"0123456789abcdef0123456789abcdef"),
                expires_at: NOW + 72 * 3600,
                totp_confirmed: false,
            }),
        },
    }
}

/// [AC-15.3] [VLD-ANB-MIDGARD-0002] An approved but unactivated record cannot log in,
/// whatever is presented, and is refused exactly like a wrong password; once the
/// principal has activated it, it logs in with both factors.
#[test]
fn an_unactivated_record_refuses_login_with_the_same_401() {
    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();
    let hasher = Pbkdf2Hasher::new(1000);
    let mut dir = InMemoryDirectory::new();
    dir.enroll("dana", unactivated(&hasher));
    let totp = TotpSecret::new(vec![7u8; 20]);

    let attempt = |dir: &InMemoryDirectory, password: &[u8]| {
        Authenticator::new(&idp, dir, &hasher).authenticate(
            "dana",
            password,
            MfaProof::Totp(totp.code_at(NOW)),
            Purpose("investigation".into()),
            NOW,
            TTL,
        )
    };
    // The empty password matches the empty hash in no hasher; the activation code is
    // not a password either.
    for presented in [&b""[..], b"0123456789abcdef0123456789abcdef", b"anything"] {
        assert_eq!(attempt(&dir, presented), Err(AuthError::BadPassword));
    }

    // A record with a password set but the activation still pending is refused too.
    let mut half = unactivated(&hasher);
    half.credentials.password_hash = hasher.hash(b"a password of twelve");
    dir.enroll("dana", half.clone());
    assert_eq!(
        attempt(&dir, b"a password of twelve"),
        Err(AuthError::BadPassword)
    );

    // Activated: the activation is gone and both factors log in.
    half.credentials.activation = None;
    assert!(dir.update_credentials("dana", half.credentials));
    assert!(attempt(&dir, b"a password of twelve").is_ok());
}

/// The original, untagged record encoding, kept here to write a directory as an
/// earlier build did.
fn encode_v1(subject: &str, e: &Enrollment) -> Vec<u8> {
    fn put(o: &mut Vec<u8>, b: &[u8]) {
        o.extend_from_slice(&(b.len() as u32).to_le_bytes());
        o.extend_from_slice(b);
    }
    let mut o = Vec::new();
    put(&mut o, subject.as_bytes());
    o.extend_from_slice(&(e.user.roles.len() as u32).to_le_bytes());
    for r in &e.user.roles {
        put(&mut o, r.0.as_bytes());
    }
    o.push(e.user.clearance.level.0);
    o.extend_from_slice(&(e.user.clearance.compartments.len() as u32).to_le_bytes());
    for c in &e.user.clearance.compartments {
        put(&mut o, c.0.as_bytes());
    }
    o.extend_from_slice(&(e.user.purposes.len() as u32).to_le_bytes());
    for p in &e.user.purposes {
        put(&mut o, p.0.as_bytes());
    }
    put(&mut o, e.credentials.password_hash.as_bytes());
    match &e.credentials.totp {
        Some(t) => {
            o.push(1);
            put(&mut o, t.secret());
        }
        None => o.push(0),
    }
    o.extend_from_slice(&0u32.to_le_bytes()); // no passkeys
    o
}

/// [AC-15.2] [VLD-ANB-MIDGARD-0002] A directory written by an earlier build opens,
/// its entries without a pending activation; what is written now is the tagged
/// version-2 record, carrying the activation across a restart.
#[test]
fn the_directory_reads_v1_records_and_writes_v2() {
    let dir_path = std::env::temp_dir().join(format!("iam-rc29-v1v2-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&dir_path);
    let hasher = Pbkdf2Hasher::new(1000);
    let legacy = Enrollment {
        user: analyst_user(),
        credentials: Credentials {
            password_hash: hasher.hash(b"pw"),
            totp: Some(TotpSecret::new(vec![9u8; 20])),
            passkeys: vec![],
            activation: None,
        },
    };
    {
        // An earlier build's directory: one untagged record in the same journal.
        let (mut journal, _) =
            audit_writer::SealedJournal::open(&dir_path, sealer(5), b"iam-directory-v1").unwrap();
        journal.append(&encode_v1("erin", &legacy)).unwrap();
    }

    {
        let mut dir = DurableDirectory::open(&dir_path, sealer(5)).expect("a v1 directory opens");
        let entry = dir.get("erin").expect("the v1 entry is read");
        assert_eq!(
            entry, &legacy,
            "every v1 field survives and activation defaults"
        );
        assert!(entry.credentials.activation.is_none());
        dir.enroll("dana", unactivated(&hasher)).unwrap();
    }

    let records = {
        let (_, records) =
            audit_writer::SealedJournal::open(&dir_path, sealer(5), b"iam-directory-v1").unwrap();
        records
    };
    assert_eq!(records.len(), 2);
    assert_ne!(records[0][0], 0xFF, "the earlier record is untagged");
    assert_eq!(
        &records[1][..3],
        &[0xFF, 2, 0],
        "the new record is tagged v2"
    );

    let dir = DurableDirectory::open(&dir_path, sealer(5)).unwrap();
    assert_eq!(dir.get("erin"), Some(&legacy));
    assert_eq!(
        dir.get("dana"),
        Some(&unactivated_with(&hasher, dir.get("dana")))
    );
    let activation = dir.get("dana").unwrap().credentials.activation.as_ref();
    let activation = activation.expect("the activation survives a restart");
    assert_eq!(activation.expires_at, NOW + 72 * 3600);
    assert!(!activation.totp_confirmed);
    assert!(hasher.verify(b"0123456789abcdef0123456789abcdef", &activation.code_hash));

    let _ = std::fs::remove_file(&dir_path);
}

/// `unactivated` with the hash just read back (a fresh hash has a fresh salt).
fn unactivated_with(hasher: &Pbkdf2Hasher, read: Option<&Enrollment>) -> Enrollment {
    let mut expected = unactivated(hasher);
    expected.credentials.activation.as_mut().unwrap().code_hash = read
        .and_then(|e| e.credentials.activation.as_ref())
        .map(|a| a.code_hash.clone())
        .unwrap_or_default();
    expected
}
