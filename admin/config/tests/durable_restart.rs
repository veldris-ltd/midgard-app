//! RC-14 (config): the signed config history survives a restart and re-verifies.

use std::path::PathBuf;

use admin_config::{ConfigState, DurableConfigStore};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{Rng, SigId, SignatureScheme};

const ALG: SigId = SigId::MlDsa65;

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
    let d = std::env::temp_dir().join(format!("admin-config-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let p = fips_provider();
    let root = SoftwareTrustRoot::generate(&p, "cfg-root", &mut SeedRng(9)).unwrap();
    KmsSealer::new(Kms::new(p, root))
}

fn open(path: &PathBuf) -> DurableConfigStore {
    let (vk, sk) = FipsSig::new(ALG).keypair(&mut SeedRng(500)).unwrap();
    DurableConfigStore::open(
        path,
        Box::new(sealer()),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk,
    )
    .unwrap()
}

#[test]
fn config_history_survives_restart_and_reverifies() {
    let dir = scratch();
    let path = dir.join("config.journal");

    {
        let mut store = open(&path);
        store
            .commit(
                ConfigState::new().with("log.level", "info"),
                "alice",
                "init",
            )
            .unwrap();
        store
            .commit(
                ConfigState::new()
                    .with("log.level", "debug")
                    .with("region", "emea"),
                "bob",
                "tune",
            )
            .unwrap();
        store.rollback(0, "carol", "revert to init").unwrap();
        assert_eq!(store.history().len(), 3);
    }

    // At rest: config values are ciphertext.
    let on_disk = std::fs::read(&path).unwrap();
    for n in [b"log.level".as_slice(), b"debug", b"emea"] {
        assert!(
            !on_disk.windows(n.len()).any(|w| w == n),
            "plaintext leak: {:?}",
            String::from_utf8_lossy(n)
        );
    }

    // Restart: history recovered and the signed chain re-verifies.
    let store = open(&path);
    assert_eq!(store.history().len(), 3, "config history recovered");
    store
        .verify(&fips_provider())
        .expect("recovered config chain re-verifies");
    assert_eq!(
        store
            .head()
            .unwrap()
            .state
            .entries()
            .get("log.level")
            .map(|s| s.as_str()),
        Some("info")
    );

    let _ = std::fs::remove_dir_all(&dir);
}
