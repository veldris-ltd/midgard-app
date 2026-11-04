//! Logins authenticate through `iam::Authenticator` against an identity directory.
//! Against a durable, KMS-sealed directory an enrolled principal's password and TOTP
//! second factor are both enforced, every refusal is the same 401, and the enrollment
//! survives a restart. A legacy password hash is replaced with Argon2id at the next
//! sign-in, durably. A TOTP code is accepted once, across a restart too.
//! [RC-29] [RC-37] [MG-SEC-11] [VLD-ANB-MIDGARD-0026]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use governance::Governance;
use http_gateway::http::{Request, Response};
use http_gateway::json::parse;
use http_gateway::{demo_operator, dev_login_policy, SecuredGateway, DEMO_PASSWORD, DEMO_USERNAME};
use iam::{
    Credentials, Directory, DurableDirectory, Enrollment, Idp, PasswordHasher, Pbkdf2Hasher,
    TotpSecret,
};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use policy::DefaultPdp;
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{OsRng, Rng};

const NOW: u64 = 1_800_000_000;
const ROOT_SEED: u64 = 86_420;
const PASSWORD: &str = "correct horse battery staple";

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

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "login-directory-{tag}-{}-{}",
        std::process::id(),
        uniq
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The same KMS identity on every open, as after a restart.
fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root =
        SoftwareTrustRoot::generate(&provider, "directory-root", &mut SeedRng(ROOT_SEED)).unwrap();
    KmsSealer::new(Kms::new(provider, root))
}

fn open_directory(dir: &Path) -> DurableDirectory {
    DurableDirectory::open(dir.join("identity.directory"), Box::new(sealer()))
        .expect("directory opens")
}

/// A gateway that authenticates against the durable directory in `dir`.
fn gateway(dir: &Path) -> SecuredGateway {
    let idp = Idp::generate("test-idp", &mut OsRng).expect("idp");
    let issuer_vk = idp.verifying_key().clone();
    let governance =
        Governance::new(DefaultPdp::new(dev_login_policy()), &mut OsRng).expect("governance");
    SecuredGateway::new(fips_provider(), issuer_vk, governance, Vec::new())
        .with_idp(idp, Pbkdf2Hasher::new(1_000))
        .with_directory(open_directory(dir))
}

fn login(gw: &SecuredGateway, body: &str) -> Response {
    login_at(gw, body, NOW)
}

fn login_at(gw: &SecuredGateway, body: &str, now: u64) -> Response {
    gw.handle(
        &Request {
            method: "POST".into(),
            path: "/api/v1/login".into(),
            query: String::new(),
            body: body.into(),
            authorization: None,
            origin: None,
            forwarded_for: None,
        },
        now,
    )
}

fn credentials(username: &str, password: &str, totp: Option<&str>) -> String {
    match totp {
        Some(code) => {
            format!(r#"{{"username":"{username}","password":"{password}","totp":"{code}"}}"#)
        }
        None => format!(r#"{{"username":"{username}","password":"{password}"}}"#),
    }
}

#[test]
fn a_durable_principal_needs_both_factors_and_survives_a_restart() {
    let dir = scratch("totp");
    let secret = TotpSecret::new(b"midgard-test-totp-secret".to_vec());
    {
        let mut directory = open_directory(&dir);
        directory
            .enroll(
                "alice",
                Enrollment {
                    user: demo_operator(),
                    credentials: Credentials {
                        password_hash: Pbkdf2Hasher::new(1_000).hash(PASSWORD.as_bytes()),
                        totp: Some(TotpSecret::new(secret.secret().to_vec())),
                        ..Credentials::default()
                    },
                },
            )
            .expect("enrolls");
    } // directory closed

    let good = secret.code_at(NOW);
    let wrong = {
        let n: u64 = good.parse().expect("numeric code");
        let modulus = 10u64.pow(u32::try_from(good.len()).expect("short code"));
        format!("{:0width$}", (n + 500_000) % modulus, width = good.len())
    };

    let gw = gateway(&dir);
    for (i, (label, body)) in [
        ("no second factor", credentials("alice", PASSWORD, None)),
        (
            "wrong second factor",
            credentials("alice", PASSWORD, Some(&wrong)),
        ),
        (
            "wrong password",
            credentials("alice", "tr0ub4dor", Some(&good)),
        ),
        (
            "unknown principal",
            credentials("mallory", PASSWORD, Some(&good)),
        ),
        (
            "the built-in demo credential",
            credentials(DEMO_USERNAME, DEMO_PASSWORD, None),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let r = login(&gw, &body);
        assert_eq!(r.status, 401, "{label}: {}", r.body);
        assert!(!r.body.contains("token"), "{label} mints nothing");
        // Compared with a different stranger each time, so no one name collects
        // enough failures to be locked.
        let stranger = format!("stranger-{i}");
        assert_eq!(
            r.body,
            login(&gw, &credentials(&stranger, "x", None)).body,
            "{label} is indistinguishable from any other refusal"
        );
    }

    let ok = login(&gw, &credentials("alice", PASSWORD, Some(&good)));
    assert_eq!(ok.status, 200, "{}", ok.body);
    let body = parse(&ok.body).expect("JSON");
    assert_eq!(body.str_field("subject"), Some("alice"));
    let token = body.str_field("token").unwrap_or_default().to_string();
    assert!(!token.is_empty());

    // Every attempt is in the audit ledger — each refusal against an enrolled name
    // under that name, each against any other name under its keyed hash, and the one
    // success — so a spray against an account leaves lineage. [AC-27.4]
    let audit = gw.handle(
        &Request {
            method: "GET".into(),
            path: "/api/v1/audit".into(),
            query: "limit=200".into(),
            body: String::new(),
            authorization: Some(format!("Bearer {token}")),
            origin: None,
            forwarded_for: None,
        },
        NOW,
    );
    assert_eq!(audit.status, 200, "{}", audit.body);
    let rows = |actor: &str, action: &str| {
        audit
            .body
            .matches(&format!(r#""actor":"{actor}","action":"{action}""#))
            .count()
    };
    assert_eq!(rows("alice", "LOGIN refused"), 3, "{}", audit.body);
    for unenrolled in ["mallory", "stranger-0", DEMO_USERNAME] {
        let named = format!(r#""actor":"{unenrolled}""#);
        assert!(!audit.body.contains(&named), "{}", audit.body);
    }
    // mallory, the demo name and five strangers: seven names, seven hashed actors.
    let hashed: std::collections::BTreeSet<&str> = audit
        .body
        .match_indices(r#""actor":"unknown:"#)
        .map(|(at, m)| &audit.body[at + m.len()..at + m.len() + 16])
        .collect();
    assert_eq!(hashed.len(), 7, "{}", audit.body);
    assert_eq!(rows("alice", "LOGIN succeeded"), 1, "{}", audit.body);
    drop(gw);

    // Restart: a new gateway over the reopened directory still knows alice. The code
    // already used is spent, so she logs in with the next one. [VLD-ANB-MIDGARD-0026]
    let gw = gateway(&dir);
    let next = secret.code_at(NOW + 30);
    assert_eq!(
        login(&gw, &credentials("alice", PASSWORD, Some(&next))).status,
        200
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// MG-SEC-11: a principal stored with a legacy PBKDF2 hash signs in once more and
/// leaves an Argon2id hash in the durable directory; a refused attempt changes nothing.
#[test]
fn a_legacy_password_hash_is_upgraded_at_the_next_sign_in() {
    let dir = scratch("rehash");
    let legacy = Pbkdf2Hasher::new(1_000).hash(PASSWORD.as_bytes());
    assert!(legacy.starts_with("pbkdf2-sha256$"));
    open_directory(&dir)
        .enroll(
            "bob",
            Enrollment {
                user: demo_operator(),
                credentials: Credentials {
                    password_hash: legacy.clone(),
                    ..Credentials::default()
                },
            },
        )
        .expect("enrolls");
    let stored = |dir: &Path| {
        open_directory(dir)
            .enrollment("bob")
            .expect("bob is enrolled")
            .credentials
            .password_hash
            .clone()
    };

    let gw = gateway(&dir);
    assert_eq!(
        login(&gw, &credentials("bob", "tr0ub4dor", None)).status,
        401
    );
    drop(gw);
    assert_eq!(stored(&dir), legacy, "a refused sign-in writes nothing");

    let gw = gateway(&dir);
    assert_eq!(login(&gw, &credentials("bob", PASSWORD, None)).status, 200);
    drop(gw);
    let upgraded = stored(&dir);
    assert!(upgraded.starts_with("$argon2id$"), "{upgraded}");

    // The upgraded hash survives a restart and still signs bob in.
    let gw = gateway(&dir);
    assert_eq!(login(&gw, &credentials("bob", PASSWORD, None)).status, 200);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A durable directory in `dir` holding alice with a password and `secret` as her TOTP
/// factor.
fn enrol_with_totp(dir: &Path, secret: &TotpSecret) {
    open_directory(dir)
        .enroll(
            "alice",
            Enrollment {
                user: demo_operator(),
                credentials: Credentials {
                    password_hash: Pbkdf2Hasher::new(1_000).hash(PASSWORD.as_bytes()),
                    totp: Some(secret.clone()),
                    ..Credentials::default()
                },
            },
        )
        .expect("enrolls");
}

/// [AC-57.1] [VLD-ANB-MIDGARD-0026] A TOTP code logs in once: the same code five
/// seconds later, still inside its step, is refused with the uniform 401, and the
/// next step's code logs in.
#[test]
fn a_totp_code_cannot_be_reused_within_the_window() {
    let dir = scratch("reuse");
    let secret = TotpSecret::new(b"midgard-test-totp-secret".to_vec());
    enrol_with_totp(&dir, &secret);
    let gw = gateway(&dir);
    let code = secret.code_at(NOW);

    let first = login_at(&gw, &credentials("alice", PASSWORD, Some(&code)), NOW);
    assert_eq!(first.status, 200, "{}", first.body);
    let again = login_at(&gw, &credentials("alice", PASSWORD, Some(&code)), NOW + 5);
    assert_eq!(again.status, 401, "{}", again.body);
    assert!(
        !again.body.contains("token"),
        "a replayed code mints nothing"
    );
    assert_eq!(
        again.body,
        login_at(&gw, &credentials("stranger", "x", None), NOW + 5).body,
        "a replay is indistinguishable from any other refusal"
    );

    let next = secret.code_at(NOW + 30);
    let ok = login_at(&gw, &credentials("alice", PASSWORD, Some(&next)), NOW + 5);
    assert_eq!(ok.status, 200, "{}", ok.body);

    let _ = std::fs::remove_dir_all(&dir);
}

/// [AC-57.2] [VLD-ANB-MIDGARD-0026] The step a login accepted is in the durable
/// directory, so a gateway reopened over the same state still refuses the code.
#[test]
fn the_last_accepted_totp_step_survives_a_restart() {
    let dir = scratch("step-restart");
    let secret = TotpSecret::new(b"midgard-test-totp-secret".to_vec());
    enrol_with_totp(&dir, &secret);
    let code = secret.code_at(NOW);

    let gw = gateway(&dir);
    let ok = login(&gw, &credentials("alice", PASSWORD, Some(&code)));
    assert_eq!(ok.status, 200, "{}", ok.body);
    drop(gw);

    let stored = open_directory(&dir)
        .enrollment("alice")
        .expect("alice is enrolled")
        .credentials
        .totp_last_step;
    assert_eq!(stored, Some(NOW / 30));

    let gw = gateway(&dir);
    let replay = login(&gw, &credentials("alice", PASSWORD, Some(&code)));
    assert_eq!(replay.status, 401, "{}", replay.body);

    let _ = std::fs::remove_dir_all(&dir);
}
