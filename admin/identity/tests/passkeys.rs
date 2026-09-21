//! Admin passkeys at rest: the registry of public passkeys survives a restart, an
//! admin's own passkey rebuilt from stored key material signs in against it, and a
//! credential that is the wrong algorithm, a mismatched pair or a second registration
//! for the same admin is refused. [RC-30] [RC-14]

use std::path::PathBuf;

use admin_identity::{
    AdminChallenge, AdminId, DurableAdminPasskeys, IdentityError, RegisteredPasskey,
    SoftwareAdminAuthenticator, PASSKEY_SIG_ALG,
};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{Rng, SecretBytes, SigId, SignatureScheme, SigningKey};

const NOW: u64 = 1_000;

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

fn scratch() -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let d = std::env::temp_dir().join(format!("admin-passkeys-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let p = fips_provider();
    let root = SoftwareTrustRoot::generate(&p, "passkey-root", &mut SeedRng(11)).unwrap();
    KmsSealer::new(Kms::new(p, root))
}

fn open(path: &PathBuf) -> DurableAdminPasskeys {
    DurableAdminPasskeys::open(path, Box::new(sealer()), fips_provider()).unwrap()
}

#[test]
fn a_registered_passkey_signs_in_after_a_restart() {
    let dir = scratch();
    let path = dir.join("passkeys.journal");

    // The admin's keypair, as their sealed keystore would hold it.
    let (vk, sk) = FipsSig::new(PASSKEY_SIG_ALG)
        .keypair(&mut SeedRng(21))
        .unwrap();
    let sk_bytes = sk.expose().to_vec();

    {
        let mut registry = open(&path);
        registry
            .register(
                AdminId("alice".into()),
                RegisteredPasskey {
                    credential_id: "alice-passkey".into(),
                    public_key: vk.clone(),
                },
            )
            .unwrap();
    }

    let registry = open(&path);
    assert!(registry.is_registered(&AdminId("alice".into())));
    assert_eq!(registry.admins(), &[AdminId("alice".into())]);

    let device = SoftwareAdminAuthenticator::from_key(
        &fips_provider(),
        "alice-passkey",
        vk,
        SigningKey::new(PASSKEY_SIG_ALG, SecretBytes::new(sk_bytes)),
    )
    .expect("the stored passkey rebuilds");
    let challenge = AdminChallenge::sample(&mut SeedRng(31));
    let assertion = device
        .authenticate(&fips_provider(), &challenge, true)
        .unwrap();
    let session = registry
        .authenticator()
        .sign_in(&AdminId("alice".into()), &challenge, &assertion, NOW)
        .expect("a restarted registry still authenticates the admin");
    assert!(session.has_mfa());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_wrong_or_repeated_credential_is_refused() {
    let dir = scratch();
    let mut registry = open(&dir.join("passkeys.journal"));

    let (vk_a, _) = FipsSig::new(PASSKEY_SIG_ALG)
        .keypair(&mut SeedRng(41))
        .unwrap();
    let (_, sk_b) = FipsSig::new(PASSKEY_SIG_ALG)
        .keypair(&mut SeedRng(42))
        .unwrap();
    assert!(
        SoftwareAdminAuthenticator::from_key(&fips_provider(), "mixed", vk_a.clone(), sk_b)
            .is_err(),
        "halves of two different keypairs"
    );

    let (vk_87, sk_87) = FipsSig::new(SigId::MlDsa87)
        .keypair(&mut SeedRng(43))
        .unwrap();
    assert!(
        SoftwareAdminAuthenticator::from_key(&fips_provider(), "wrong-alg", vk_87.clone(), sk_87)
            .is_err(),
        "a passkey of another algorithm"
    );
    assert_eq!(
        registry.register(
            AdminId("bob".into()),
            RegisteredPasskey {
                credential_id: "bob".into(),
                public_key: vk_87,
            },
        ),
        Err(IdentityError::WebAuthnRejected)
    );

    let passkey = RegisteredPasskey {
        credential_id: "carol".into(),
        public_key: vk_a,
    };
    registry
        .register(AdminId("carol".into()), passkey.clone())
        .unwrap();
    assert!(
        registry.register(AdminId("carol".into()), passkey).is_err(),
        "a second registration would silently replace the credential"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
