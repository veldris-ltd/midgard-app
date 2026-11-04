//! RC-29 — Given a user, when they authenticate, then credentials + MFA are verified
//! before a PQ-signed token is minted; the directory survives restart; federation
//! works when configured. A passkey's signature counter is recorded and survives a
//! restart. [RC-29] [VLD-ANB-MIDGARD-0084]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;

use iam::{
    federated_login, verify_token, Activation, AuthAssertion, AuthError, Authenticator, ClientData,
    Credentials, Directory, DurableDirectory, Enrollment, Idp, InMemoryDirectory, MfaProof,
    PasswordHasher, Pbkdf2Hasher, PqFederation, TotpSecret, UserRecord, WebAuthnCredential,
    WebAuthnPolicy, PASSKEY_SIG_ALG,
};
use policy::{ClassificationLevel, Clearance, Purpose, Role};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, SigId, SigningKey};

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
                totp_last_step: None,
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
                    totp_last_step: None,
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
            totp_last_step: None,
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
/// record (version 3 since WP-57, 4 since WP-58), carrying the activation across a
/// restart.
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
            totp_last_step: None,
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
        &[0xFF, 4, 0],
        "the new record is tagged v4"
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

/// An analyst with a password and a TOTP factor, nothing yet accepted.
fn with_totp(hasher: &Pbkdf2Hasher, totp: &TotpSecret) -> Enrollment {
    Enrollment {
        user: analyst_user(),
        credentials: Credentials {
            password_hash: hasher.hash(b"correct horse battery staple"),
            totp: Some(totp.clone()),
            ..Credentials::default()
        },
    }
}

/// [AC-57.4] [VLD-ANB-MIDGARD-0026] A login refused for its password never consumes
/// the TOTP step it carried: the same code then logs in with the right password.
#[test]
fn a_wrong_password_does_not_consume_the_totp_step() {
    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();
    let hasher = Pbkdf2Hasher::new(1000);
    let totp = TotpSecret::new(b"a-shared-totp-secret-32-bytes-xx".to_vec());
    let mut dir = InMemoryDirectory::new();
    dir.enroll("frank", with_totp(&hasher, &totp));

    let attempt = |dir: &InMemoryDirectory, password: &[u8]| {
        Authenticator::new(&idp, dir, &hasher).authenticate_full(
            "frank",
            password,
            MfaProof::Totp(totp.code_at(NOW)),
            Purpose("investigation".into()),
            NOW,
            TTL,
        )
    };
    assert_eq!(attempt(&dir, b"wrong"), Err(AuthError::BadPassword));
    let entry = dir.enrollment("frank").unwrap();
    assert_eq!(
        entry.credentials.totp_last_step, None,
        "nothing was accepted"
    );

    let outcome = attempt(&dir, b"correct horse battery staple").expect("the code still logs in");
    assert_eq!(outcome.totp_step, Some(NOW / 30));
    assert!(verify_token(idp.provider(), idp.verifying_key(), &outcome.token, NOW).is_ok());
}

/// [AC-57.1] [VLD-ANB-MIDGARD-0026] Once the step a code matched is recorded, the
/// authenticator refuses that code as a bad second factor, and the next step's code
/// logs in and reports the next step.
#[test]
fn a_reused_code_is_refused_by_the_authenticator() {
    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();
    let hasher = Pbkdf2Hasher::new(1000);
    let totp = TotpSecret::new(b"a-shared-totp-secret-32-bytes-xx".to_vec());
    let mut dir = InMemoryDirectory::new();
    dir.enroll("frank", with_totp(&hasher, &totp));

    let attempt = |dir: &InMemoryDirectory, code: String, now: u64| {
        Authenticator::new(&idp, dir, &hasher).authenticate_full(
            "frank",
            b"correct horse battery staple",
            MfaProof::Totp(code),
            Purpose("investigation".into()),
            now,
            TTL,
        )
    };
    let first = attempt(&dir, totp.code_at(NOW), NOW).expect("the first use logs in");
    let step = first.totp_step.expect("a TOTP login reports its step");
    assert_eq!(step, NOW / 30);
    assert!(dir.record_totp_step("frank", step));

    assert_eq!(
        attempt(&dir, totp.code_at(NOW), NOW + 5),
        Err(AuthError::BadMfa),
        "the same code is refused"
    );
    let next = attempt(&dir, totp.code_at(NOW + 30), NOW + 5).expect("the next code logs in");
    assert_eq!(next.totp_step, Some(step + 1));
}

/// [AC-57.2] [VLD-ANB-MIDGARD-0026] A directory written by the previous build (tagged
/// version 2) opens with no last accepted step. A recorded step is a small tagged
/// record that survives a restart; an older step writes nothing, and replacing the
/// credentials keeps the step.
#[test]
fn the_directory_reads_v2_records_and_replays_totp_steps() {
    let dir_path = std::env::temp_dir().join(format!("iam-rc29-v2v3-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&dir_path);
    let hasher = Pbkdf2Hasher::new(1000);
    let totp = TotpSecret::new(vec![9u8; 20]);
    let legacy = with_totp(&hasher, &totp);
    let records = || {
        let (_, records) =
            audit_writer::SealedJournal::open(&dir_path, sealer(6), b"iam-directory-v1").unwrap();
        records
    };
    {
        // The previous build's record: tagged version 2, no pending activation.
        let (mut journal, _) =
            audit_writer::SealedJournal::open(&dir_path, sealer(6), b"iam-directory-v1").unwrap();
        let mut v2 = vec![0xFF, 2, 0];
        v2.extend_from_slice(&encode_v1("erin", &legacy));
        v2.push(0);
        journal.append(&v2).unwrap();
    }

    {
        let mut dir = DurableDirectory::open(&dir_path, sealer(6)).expect("a v2 directory opens");
        assert_eq!(dir.get("erin"), Some(&legacy), "every v2 field survives");
        assert!(dir.record_totp_step("erin", NOW / 30).unwrap());
        assert!(dir.record_totp_step("erin", NOW / 30 - 1).unwrap());
        assert!(!dir.record_totp_step("nobody", NOW / 30).unwrap());
    }
    let written = records();
    assert_eq!(written.len(), 2, "an older step writes nothing");
    assert_eq!(
        &written[1][..3],
        &[0xFF, 3, 1],
        "a step record is tagged v3 kind 1"
    );

    {
        let mut dir = DurableDirectory::open(&dir_path, sealer(6)).unwrap();
        let entry = dir.get("erin").unwrap();
        assert_eq!(entry.credentials.totp_last_step, Some(NOW / 30));
        let mut credentials = legacy.credentials.clone();
        credentials.password_hash = hasher.hash(b"a new password");
        assert!(dir.update_credentials("erin", credentials).unwrap());
        assert_eq!(
            dir.get("erin").unwrap().credentials.totp_last_step,
            Some(NOW / 30)
        );
    }
    let written = records();
    assert_eq!(written.len(), 3);
    assert_eq!(
        &written[2][..3],
        &[0xFF, 4, 0],
        "a full record is tagged v4 (WP-58)"
    );

    let dir = DurableDirectory::open(&dir_path, sealer(6)).unwrap();
    let entry = dir.get("erin").unwrap();
    assert_eq!(entry.credentials.totp_last_step, Some(NOW / 30));
    assert!(hasher.verify(b"a new password", &entry.credentials.password_hash));

    let _ = std::fs::remove_file(&dir_path);
}

/// The origin the passkey tests' relying party serves.
const ORIGIN: &str = "https://console.enclosure.local";

/// An analyst with a password and one passkey, no assertion yet accepted, and the
/// passkey's signing key.
fn with_passkey(hasher: &Pbkdf2Hasher) -> (Enrollment, SigningKey) {
    let provider = fips_provider();
    let sig = provider.sig_for(PASSKEY_SIG_ALG).unwrap();
    let (vk, sk) = sig.keypair(&mut SeedRng(58)).unwrap();
    let enrollment = Enrollment {
        user: analyst_user(),
        credentials: Credentials {
            password_hash: hasher.hash(b"correct horse battery staple"),
            passkeys: vec![WebAuthnCredential {
                credential_id: b"grace-key".to_vec(),
                public_key: vk,
                rp_id: "console.enclosure.local".into(),
                sign_count: 0,
            }],
            ..Credentials::default()
        },
    };
    (enrollment, sk)
}

/// An assertion for `challenge` from `ORIGIN`, user present and verified, presenting
/// signature counter `count`.
fn passkey_assertion(
    cred: &WebAuthnCredential,
    sk: &SigningKey,
    challenge: &[u8],
    count: u32,
) -> AuthAssertion {
    let provider = fips_provider();
    let sig = provider.sig_for(PASSKEY_SIG_ALG).unwrap();
    let client_data_json = WebAuthnCredential::client_data_json(challenge, ORIGIN);
    let authenticator_data = cred.authenticator_data(0x01 | 0x04, count);
    let payload = WebAuthnCredential::signed_payload(&authenticator_data, &client_data_json);
    AuthAssertion {
        credential_id: cred.credential_id.clone(),
        authenticator_data,
        client_data_json,
        signature: sig.sign(sk, &payload).unwrap(),
    }
}

/// The client data of [`WebAuthnCredential::client_data_json`], read back as the edge's
/// parser would (`iam` has no JSON parser of its own). [VLD-ANB-MIDGARD-0084]
fn parse_client_data(raw: &[u8]) -> Result<ClientData, String> {
    let raw = std::str::from_utf8(raw).map_err(|e| e.to_string())?;
    let rest = raw
        .strip_prefix(r#"{"type":""#)
        .and_then(|r| r.strip_suffix(r#""}"#))
        .ok_or("not client data")?;
    let (type_, rest) = rest
        .split_once(r#"","challenge":""#)
        .ok_or("no challenge")?;
    let (challenge, origin) = rest.split_once(r#"","origin":""#).ok_or("no origin")?;
    Ok(ClientData {
        type_: type_.into(),
        challenge: iam::enc::unb64url(challenge).ok_or("challenge is not base64url")?,
        origin: origin.into(),
    })
}

/// [AC-58.5] [VLD-ANB-MIDGARD-0084] A recorded signature counter is a small tagged
/// record that is replayed on reopening; a lower counter writes nothing, an unknown
/// passkey records nothing, and replacing the credentials keeps the counter. A
/// version 3 enrollment, written before counters existed, opens with counter 0.
#[test]
fn a_passkey_sign_count_survives_a_restart() {
    let dir_path = std::env::temp_dir().join(format!("iam-rc29-v4-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&dir_path);
    let hasher = Pbkdf2Hasher::new(1000);
    let (grace, _) = with_passkey(&hasher);
    let records = || {
        let (_, records) =
            audit_writer::SealedJournal::open(&dir_path, sealer(9), b"iam-directory-v1").unwrap();
        records
    };
    {
        // The previous build's record: tagged version 3, no activation, no step, and
        // no passkey counters.
        let (mut journal, _) =
            audit_writer::SealedJournal::open(&dir_path, sealer(9), b"iam-directory-v1").unwrap();
        fn put(o: &mut Vec<u8>, b: &[u8]) {
            o.extend_from_slice(&(b.len() as u32).to_le_bytes());
            o.extend_from_slice(b);
        }
        let mut v3 = vec![0xFF, 3, 0];
        v3.extend_from_slice(&encode_v1("grace", &grace));
        // encode_v1 writes no passkeys: replace its count of 0 with this one passkey.
        v3.truncate(v3.len() - 4);
        v3.extend_from_slice(&1u32.to_le_bytes());
        let key = &grace.credentials.passkeys[0];
        put(&mut v3, &key.credential_id);
        v3.push(key.public_key.alg().as_u8());
        put(&mut v3, key.public_key.as_bytes());
        put(&mut v3, key.rp_id.as_bytes());
        v3.extend_from_slice(&[0, 0]); // no activation, no TOTP step
        journal.append(&v3).unwrap();
    }

    {
        let mut dir = DurableDirectory::open(&dir_path, sealer(9)).expect("a v3 directory opens");
        assert_eq!(dir.get("grace"), Some(&grace), "every v3 field survives");
        assert!(dir.record_passkey_count("grace", b"grace-key", 7).unwrap());
        assert!(dir.record_passkey_count("grace", b"grace-key", 5).unwrap());
        assert!(!dir.record_passkey_count("grace", b"other-key", 9).unwrap());
        assert!(!dir.record_passkey_count("nobody", b"grace-key", 9).unwrap());
    }
    let written = records();
    assert_eq!(written.len(), 2, "a lower counter writes nothing");
    assert_eq!(
        &written[1][..3],
        &[0xFF, 4, 2],
        "a counter record is tagged v4 kind 2"
    );

    {
        let mut dir = DurableDirectory::open(&dir_path, sealer(9)).unwrap();
        let entry = dir.get("grace").unwrap();
        assert_eq!(
            entry.credentials.passkeys[0].sign_count, 7,
            "the counter is replayed"
        );
        let mut credentials = grace.credentials.clone();
        credentials.password_hash = hasher.hash(b"a new password");
        assert!(dir.update_credentials("grace", credentials).unwrap());
        assert_eq!(
            dir.get("grace").unwrap().credentials.passkeys[0].sign_count,
            7
        );
    }
    let written = records();
    assert_eq!(
        &written[2][..3],
        &[0xFF, 4, 0],
        "a full record is tagged v4"
    );

    let dir = DurableDirectory::open(&dir_path, sealer(9)).unwrap();
    let entry = dir.get("grace").unwrap();
    assert_eq!(
        entry.credentials.passkeys[0].sign_count, 7,
        "the full record keeps it"
    );
    assert!(hasher.verify(b"a new password", &entry.credentials.password_hash));

    let _ = std::fs::remove_file(&dir_path);
}

/// [AC-58.5] [VLD-ANB-MIDGARD-0084] A passkey login through the authenticator reports
/// the credential and counter it accepted; once recorded, the same assertion is
/// refused as a bad second factor and a later counter logs in. With the default
/// policy, which names no origin, no assertion verifies.
#[test]
fn a_passkey_login_through_the_authenticator_records_the_counter() {
    let idp = Idp::generate("enclave-idp", &mut SeedRng(1)).unwrap();
    let hasher = Pbkdf2Hasher::new(1000);
    let (grace, sk) = with_passkey(&hasher);
    let cred = grace.credentials.passkeys[0].clone();
    let mut dir = InMemoryDirectory::new();
    dir.enroll("grace", grace);
    let policy = WebAuthnPolicy {
        origins: vec![ORIGIN.into()],
        require_uv: true,
    };
    let challenge = b"a-fresh-login-challenge";

    let attempt = |dir: &InMemoryDirectory, policy: WebAuthnPolicy, count: u32| {
        Authenticator::new(&idp, dir, &hasher)
            .with_webauthn_policy(policy)
            .authenticate_full(
                "grace",
                b"correct horse battery staple",
                MfaProof::WebAuthn {
                    assertion: passkey_assertion(&cred, &sk, challenge, count),
                    challenge,
                    parse_client_data: &parse_client_data,
                },
                Purpose("investigation".into()),
                NOW,
                TTL,
            )
    };
    assert_eq!(
        attempt(&dir, WebAuthnPolicy::default(), 3),
        Err(AuthError::BadMfa),
        "the default policy accepts no origin"
    );

    let first = attempt(&dir, policy.clone(), 3).expect("the passkey logs in");
    assert_eq!(first.passkey_count, Some((b"grace-key".to_vec(), 3)));
    assert_eq!(first.totp_step, None);
    assert!(verify_token(idp.provider(), idp.verifying_key(), &first.token, NOW).is_ok());
    assert!(dir.record_passkey_count("grace", b"grace-key", 3));
    assert_eq!(
        dir.enrollment("grace").unwrap().credentials.passkeys[0].sign_count,
        3
    );

    assert_eq!(
        attempt(&dir, policy.clone(), 3),
        Err(AuthError::BadMfa),
        "the same counter is refused"
    );
    let next = attempt(&dir, policy, 4).expect("a later counter logs in");
    assert_eq!(next.passkey_count, Some((b"grace-key".to_vec(), 4)));
}
