//! RC-14 (identity): role bindings survive a restart, recovered intact.

use std::path::PathBuf;

use admin_identity::{
    AdminId, DurableIdentityDirectory, ResourceScope, RoleBinding, RoleCatalog,
    ROLE_SECURITY_OFFICER,
};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use pqc_agility::backend::fips::provider as fips_provider;
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

fn scratch() -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let d = std::env::temp_dir().join(format!("admin-identity-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let p = fips_provider();
    let root = SoftwareTrustRoot::generate(&p, "id-root", &mut SeedRng(9)).unwrap();
    KmsSealer::new(Kms::new(p, root))
}

fn open(path: &PathBuf) -> DurableIdentityDirectory {
    DurableIdentityDirectory::open(path, Box::new(sealer()), RoleCatalog::standard()).unwrap()
}

#[test]
fn identity_bindings_survive_restart() {
    let dir = scratch();
    let path = dir.join("identity.journal");
    let kms_scope = ResourceScope::all("kms");

    {
        let mut store = open(&path);
        store
            .bind(RoleBinding {
                admin: AdminId("alice".into()),
                role: ROLE_SECURITY_OFFICER.into(),
                scope: kms_scope.clone(),
                not_before: 0,
                not_after: 1_000_000,
            })
            .unwrap();
        store
            .bind(RoleBinding {
                admin: AdminId("bob".into()),
                role: ROLE_SECURITY_OFFICER.into(),
                scope: kms_scope.clone(),
                not_before: 0,
                not_after: 500,
            })
            .unwrap();
        assert!(store.directory().holds_role(
            &AdminId("alice".into()),
            ROLE_SECURITY_OFFICER,
            &kms_scope,
            100
        ));
    }

    let on_disk = std::fs::read(&path).unwrap();
    assert!(
        !on_disk.windows(5).any(|w| w == b"alice"),
        "binding subject leaked in plaintext"
    );

    // Restart: bindings recovered with their scope + validity windows intact.
    let store = open(&path);
    let d = store.directory();
    assert!(
        d.holds_role(
            &AdminId("alice".into()),
            ROLE_SECURITY_OFFICER,
            &kms_scope,
            100
        ),
        "alice binding recovered"
    );
    // bob's binding expires at 500 — active at 100, expired at 600.
    assert!(d.holds_role(
        &AdminId("bob".into()),
        ROLE_SECURITY_OFFICER,
        &kms_scope,
        100
    ));
    assert!(
        !d.holds_role(
            &AdminId("bob".into()),
            ROLE_SECURITY_OFFICER,
            &kms_scope,
            600
        ),
        "expiry window recovered"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
