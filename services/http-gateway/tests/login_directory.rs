//! Logins authenticate through `iam::Authenticator` against an identity directory.
//! Against a durable, KMS-sealed directory an enrolled principal's password and TOTP
//! second factor are both enforced, every refusal is the same 401, and the enrollment
//! survives a restart. [RC-29] [RC-37]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use governance::Governance;
use http_gateway::http::{Request, Response};
use http_gateway::json::parse;
use http_gateway::{demo_operator, dev_login_policy, SecuredGateway, DEMO_PASSWORD, DEMO_USERNAME};
use iam::{
    Credentials, DurableDirectory, Enrollment, Idp, PasswordHasher, Pbkdf2Hasher, TotpSecret,
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
    gw.handle(
        &Request {
            method: "POST".into(),
            path: "/api/v1/login".into(),
            query: String::new(),
            body: body.into(),
            authorization: None,
            origin: None,
        },
        NOW,
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
    for (label, body) in [
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
    ] {
        let r = login(&gw, &body);
        assert_eq!(r.status, 401, "{label}: {}", r.body);
        assert!(!r.body.contains("token"), "{label} mints nothing");
        assert_eq!(
            r.body,
            login(&gw, &credentials("mallory", "x", None)).body,
            "{label} is indistinguishable from any other refusal"
        );
    }

    let ok = login(&gw, &credentials("alice", PASSWORD, Some(&good)));
    assert_eq!(ok.status, 200, "{}", ok.body);
    let body = parse(&ok.body).expect("JSON");
    assert_eq!(body.str_field("subject"), Some("alice"));
    let token = body.str_field("token").unwrap_or_default().to_string();
    assert!(!token.is_empty());

    // Every attempt is in the audit ledger — each refusal under the name it tried, and
    // the one success — so a spray against an account leaves lineage.
    let audit = gw.handle(
        &Request {
            method: "GET".into(),
            path: "/api/v1/audit".into(),
            query: "limit=200".into(),
            body: String::new(),
            authorization: Some(format!("Bearer {token}")),
            origin: None,
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
    assert_eq!(rows("mallory", "LOGIN refused"), 6, "{}", audit.body);
    assert_eq!(rows(DEMO_USERNAME, "LOGIN refused"), 1, "{}", audit.body);
    assert_eq!(rows("alice", "LOGIN succeeded"), 1, "{}", audit.body);
    drop(gw);

    // Restart: a new gateway over the reopened directory still knows alice.
    let gw = gateway(&dir);
    assert_eq!(
        login(&gw, &credentials("alice", PASSWORD, Some(&good))).status,
        200
    );

    let _ = std::fs::remove_dir_all(&dir);
}
