//! RC-29 — Given a user, when they authenticate, then credentials + MFA are verified
//! before a PQ-signed token is minted; the directory survives restart; federation
//! works when configured. [RC-29]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;

use iam::{
    federated_login, verify_token, AuthError, Authenticator, Credentials, DurableDirectory,
    Enrollment, Idp, MfaProof, PasswordHasher, Pbkdf2Hasher, PqFederation, TotpSecret, UserRecord,
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
    let federation = PqFederation::new(fips_provider(), up_vk.clone(), SigId::MlDsa65);

    // The upstream vouches for a federated analyst.
    let claims = iam::FederatedClaims {
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
        &federation,
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
        &federation,
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
        &federation,
        &assertion2,
        Purpose("investigation".into()),
        NOW,
        TTL
    )
    .is_err());
}
