//! RC-12 (fleet half): fleet state is durable. Given channel desires and
//! installation check-ins, when Bifrost restarts, both are recovered (and
//! re-authenticated on unseal); a tampered on-disk record fails closed.
//! [FR-B-3] [RC-12]

use std::path::PathBuf;

use bifrost_fleet::{AgentReport, CheckIn, DurableFleet, Installation, NodeStatus};
use bifrost_rollout::VersionId;
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

const ROOT_SEED: u64 = 8642;

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
        "bifrost-fleet-{tag}-{}-{}",
        std::process::id(),
        uniq
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root =
        SoftwareTrustRoot::generate(&provider, "fleet-root", &mut SeedRng(ROOT_SEED)).unwrap();
    KmsSealer::new(Kms::new(provider, root))
}

fn open(path: &PathBuf) -> DurableFleet {
    DurableFleet::open(path, Box::new(sealer()), "bifrost-fleet").unwrap()
}

#[test]
fn fleet_state_survives_restart() {
    let dir = scratch("ok");
    let path = dir.join("fleet.journal");

    // --- session 1: declare a channel, register installs, record a check-in ---
    {
        let mut fleet = open(&path);
        fleet
            .set_channel("stable", VersionId::new("1.4.0"))
            .unwrap();
        fleet
            .register(Installation::new(
                "host-a",
                "stable",
                VersionId::new("1.4.0"),
            ))
            .unwrap();
        fleet
            .register(Installation::new(
                "host-b",
                "stable",
                VersionId::new("1.3.0"),
            ))
            .unwrap();
        // host-b checks in on the new version (converges).
        fleet
            .mark_updated("host-b", VersionId::new("1.4.0"))
            .unwrap();
        // host-c is still behind.
        fleet
            .register(Installation::new(
                "host-c",
                "stable",
                VersionId::new("1.2.0"),
            ))
            .unwrap();

        let drift = fleet.fleet().drift();
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].install_id, "host-c");
    }

    // On-disk journal carries no plaintext channel/version identifiers.
    let on_disk = std::fs::read(&path).unwrap();
    for needle in [b"stable".as_slice(), b"host-c", b"1.4.0"] {
        assert!(
            !on_disk.windows(needle.len()).any(|w| w == needle),
            "plaintext {:?} must not be on disk",
            String::from_utf8_lossy(needle)
        );
    }

    // --- restart: reopen; channel desires + install actuals recovered ---
    let fleet = open(&path);
    assert_eq!(
        fleet.fleet().channel_desired("stable"),
        Some(&VersionId::new("1.4.0"))
    );
    let drift = fleet.fleet().drift();
    assert_eq!(drift.len(), 1, "drift recomputed identically after restart");
    assert_eq!(drift[0].install_id, "host-c");
    assert_eq!(drift[0].from, VersionId::new("1.2.0"));
    assert_eq!(drift[0].to, VersionId::new("1.4.0"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn agent_check_ins_survive_restart() {
    let dir = scratch("checkin");
    let path = dir.join("fleet.journal");
    {
        let mut fleet = open(&path);
        fleet
            .set_channel("stable", VersionId::new("1.5.0"))
            .unwrap();
        fleet
            .check_in(
                AgentReport::new("host-a", "stable", VersionId::new("1.4.0"), true),
                100,
            )
            .unwrap();
        fleet
            .check_in(
                AgentReport::new("host-b", "stable", VersionId::new("1.5.0"), false),
                160,
            )
            .unwrap();
    }

    // Restart: the check-ins — version, health and last-seen — come back.
    let fleet = open(&path);
    let recovered = fleet.fleet();
    assert_eq!(
        recovered.checkin("host-a"),
        Some(&CheckIn {
            version: VersionId::new("1.4.0"),
            healthy: true,
            last_seen: 100,
        })
    );
    assert_eq!(recovered.installations().count(), 2);
    assert_eq!(recovered.node_status("host-a", 200, 60), NodeStatus::Stale);
    assert_eq!(
        recovered.node_status("host-b", 200, 60),
        NodeStatus::Unhealthy
    );
    let drift = recovered.drift();
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].install_id, "host-a");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tampered_fleet_journal_fails_closed() {
    let dir = scratch("tamper");
    let path = dir.join("fleet.journal");
    {
        let mut fleet = open(&path);
        fleet
            .set_channel("stable", VersionId::new("1.4.0"))
            .unwrap();
        fleet
            .register(Installation::new(
                "host-a",
                "stable",
                VersionId::new("1.4.0"),
            ))
            .unwrap();
    }
    let mut bytes = std::fs::read(&path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    std::fs::write(&path, &bytes).unwrap();

    assert!(
        DurableFleet::open(&path, Box::new(sealer()), "bifrost-fleet").is_err(),
        "a tampered fleet journal must fail closed on load"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
