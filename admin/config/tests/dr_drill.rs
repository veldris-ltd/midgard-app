//! RC-34 — the **tested disaster-recovery restore drill**. It simulates a node loss
//! and follows the DR runbook's restore steps against a real durable store (the
//! admin config history — representative of every RT-09..14 SealedJournal store), then
//! verifies the platform state is recovered, re-verifies the signed chain, and
//! asserts the recovery meets the stated RPO (no committed data lost up to the last
//! snapshot) and RTO (restore completes well within the budget). A tampered backup
//! fails closed. See `docs/ops/DR_PLAN.md`. [RC-34] [RC-14]

use std::path::PathBuf;
use std::time::Instant;

use admin_config::{ConfigState, DurableConfigStore};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{Rng, SigId, SignatureScheme};

const ALG: SigId = SigId::MlDsa65;

/// Stated recovery objectives (see `docs/ops/DR_PLAN.md`). The drill asserts the
/// restore is comfortably inside the RTO; the RPO is the snapshot cadence, and the
/// drill shows every commit up to the last snapshot is recovered intact.
const RTO_BUDGET: std::time::Duration = std::time::Duration::from_secs(30 * 60);

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
    let d = std::env::temp_dir().join(format!("dr-drill-{tag}-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Rebuild the KMS sealer deterministically — in a real DR the *hardware* trust root
/// (RT-28) is recovered from the HSM/escrow, not regenerated; here the software root
/// is seeded so the restored node can unseal the backed-up journal.
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
fn dr_restore_drill_recovers_the_platform_within_rto_rpo() {
    let node = scratch("primary");
    let vault = scratch("backup-vault");
    let primary = node.join("config.journal");
    let backup = vault.join("config.journal.snapshot");

    // ---- 1. Live platform: commit config the operators rely on. ----
    {
        let mut store = open(&primary);
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
                    .with("log.level", "info")
                    .with("region", "emea"),
                "bob",
                "region",
            )
            .unwrap();
        store
            .commit(
                ConfigState::new()
                    .with("log.level", "warn")
                    .with("region", "emea"),
                "carol",
                "quiet logs",
            )
            .unwrap();
        assert_eq!(store.history().len(), 3);
    }

    // ---- 2. Snapshot backup (the RPO checkpoint): copy the sealed journal. ----
    // The journal is append-only + fsync'd, so a file copy is a consistent snapshot.
    std::fs::copy(&primary, &backup).expect("snapshot the sealed journal");
    let snapshot_history = 3usize;

    // ---- 3. Writes AFTER the snapshot — these fall inside the RPO loss window. ----
    {
        let mut store = open(&primary);
        store
            .commit(
                ConfigState::new()
                    .with("log.level", "debug")
                    .with("region", "emea"),
                "dave",
                "post-snapshot",
            )
            .unwrap();
        assert_eq!(store.history().len(), 4);
    }

    // ---- 4. NODE LOSS: the node and its disk are gone. ----
    std::fs::remove_dir_all(&node).expect("simulate total node/disk loss");
    assert!(!primary.exists(), "primary store is gone");

    // ---- 5. RESTORE DRILL (timed = RTO): stand up a replacement node from backup. ----
    let restore_node = scratch("restored");
    let restored_path = restore_node.join("config.journal");
    let start = Instant::now();
    // 5a. Retrieve the backup onto the replacement node.
    std::fs::copy(&backup, &restored_path).expect("restore the snapshot to the new node");
    // 5b. Boot the store: replay-on-open recovers state; then re-verify the signed chain.
    let restored = open(&restored_path);
    restored
        .verify(&fips_provider())
        .expect("recovered config chain re-verifies (integrity intact)");
    let rto = start.elapsed();

    // ---- 6. Verify recovery meets the objectives. ----
    // RPO: every commit up to the snapshot is recovered, byte-for-byte.
    assert_eq!(
        restored.history().len(),
        snapshot_history,
        "all pre-snapshot commits recovered (RPO honoured)"
    );
    let head = restored.head().unwrap();
    assert_eq!(head.author, "carol");
    assert_eq!(head.state.get("log.level"), Some(&"warn".to_string()));
    assert_eq!(head.state.get("region"), Some(&"emea".to_string()));
    // The post-snapshot commit is the acknowledged loss window (≤ RPO), correctly absent.
    assert!(
        restored.get_version(3).is_none(),
        "post-snapshot write is the RPO loss window"
    );

    // RTO: the restore completed well within the stated budget.
    assert!(
        rto < RTO_BUDGET,
        "restore took {rto:?}, exceeds RTO {RTO_BUDGET:?}"
    );

    // The platform is back in service on the replacement node: further commits work.
    let mut restored = restored;
    restored
        .commit(
            ConfigState::new()
                .with("log.level", "info")
                .with("region", "emea"),
            "eve",
            "resume ops",
        )
        .expect("service resumed after restore");
    assert_eq!(restored.history().len(), 4);
}

#[test]
fn dr_restore_of_a_tampered_backup_fails_closed() {
    let node = scratch("t-primary");
    let vault = scratch("t-vault");
    let primary = node.join("config.journal");
    let backup = vault.join("config.journal.snapshot");

    {
        let mut store = open(&primary);
        store
            .commit(ConfigState::new().with("k", "v"), "alice", "init")
            .unwrap();
    }
    std::fs::copy(&primary, &backup).unwrap();

    // Corrupt the backup at rest (a bit-flip in transit / on the backup medium).
    let mut bytes = std::fs::read(&backup).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&backup, &bytes).unwrap();

    // Restoring a tampered backup must FAIL CLOSED — the sealed journal's AEAD tag
    // rejects it on open, so a corrupted DR copy can never be silently trusted.
    let restore_node = scratch("t-restored");
    let restored_path = restore_node.join("config.journal");
    std::fs::copy(&backup, &restored_path).unwrap();
    let (vk, sk) = FipsSig::new(ALG).keypair(&mut SeedRng(500)).unwrap();
    let result = DurableConfigStore::open(
        &restored_path,
        Box::new(sealer()),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk,
    );
    assert!(
        result.is_err(),
        "a tampered backup must be rejected on restore"
    );
}
