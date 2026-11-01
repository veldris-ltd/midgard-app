//! VLD-ANB-MIDGARD-0042: a fleet check-in is bounded. Report fields are capped at
//! the decoder; a check-in must name a configured channel and the installation's
//! registered channel, and only an operator moves an installation; the journal grows
//! with changes, not with calls, and is compacted into one snapshot that recovers the
//! same fleet. [FR-B-3] [RC-12] [RC-17]

use std::path::{Path, PathBuf};

use bifrost_fleet::agent::{MAX_CHANNEL, MAX_INSTALL_ID, MAX_VERSION};
use bifrost_fleet::{AgentReport, DurableFleet, DurableFleetError, Fleet, FleetError};
use bifrost_rollout::VersionId;
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

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "bifrost-fleet-sec-{tag}-{}-{uniq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn open(path: &Path, heartbeat: u64) -> DurableFleet {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "fleet-root", &mut SeedRng(4242)).unwrap();
    DurableFleet::open(
        path,
        Box::new(KmsSealer::new(Kms::new(provider, root))),
        "bifrost-fleet",
    )
    .unwrap()
    .with_heartbeat_interval(heartbeat)
}

fn report(id: &str, channel: &str, version: &str) -> AgentReport {
    AgentReport::new(id, channel, VersionId::new(version), true)
}

/// A report's bytes with each field as given, bypassing [`AgentReport::new`].
fn raw(id: &str, channel: &str, version: &str) -> Vec<u8> {
    report(id, channel, version).encode()
}

#[test]
fn oversized_and_control_fields_are_refused_by_the_decoder() {
    // [VLD-ANB-MIDGARD-0042] [AC-45.1]
    let long_channel = "c".repeat(200);
    let long_id = "h".repeat(MAX_INSTALL_ID + 1);
    for (bytes, why) in [
        (raw("host-a", &long_channel, "1.0.0"), "field too long"),
        (raw(&long_id, "stable", "1.0.0"), "field too long"),
        (
            raw("host-a", "stable", &"v".repeat(MAX_VERSION + 1)),
            "field too long",
        ),
        (
            raw("host-a", "stable", "1.0.0\u{1b}[2J"),
            "control characters",
        ),
        (raw("host-a\n", "stable", "1.0.0"), "control characters"),
        (raw("host-a", "", "1.0.0"), "empty field"),
        (raw("", "stable", "1.0.0"), "empty field"),
    ] {
        let err = AgentReport::decode(&bytes).unwrap_err();
        assert!(err.contains(why), "{err} (expected {why})");
    }

    // A length prefix that claims the whole frame is refused before anything is read.
    let mut huge = Vec::new();
    huge.extend_from_slice(&(1u32 << 20).to_le_bytes());
    huge.extend(std::iter::repeat(b'a').take(1 << 20));
    assert_eq!(
        AgentReport::decode(&huge),
        Err("agent report: field too long")
    );

    // A report exactly at the caps decodes.
    let at_caps = AgentReport::new(
        "h".repeat(MAX_INSTALL_ID),
        "c".repeat(MAX_CHANNEL),
        VersionId::new("v".repeat(MAX_VERSION)),
        false,
    );
    assert_eq!(AgentReport::decode(&at_caps.encode()), Ok(at_caps));
}

#[test]
fn a_check_in_on_an_unconfigured_channel_is_refused() {
    // [VLD-ANB-MIDGARD-0042] [AC-45.2]
    let mut fleet = Fleet::new();
    fleet.set_channel("stable", VersionId::new("1.5.0"));
    assert_eq!(
        fleet.check_in(report("host-a", "edge", "1.5.0"), 10),
        Err(FleetError::UnknownChannel("edge".into()))
    );
    assert_eq!(fleet.installations().count(), 0, "nothing is registered");
    assert!(fleet.checkin("host-a").is_none());

    // The durable fleet journals nothing for it.
    let dir = scratch("unknown");
    let mut durable = open(&dir.join("fleet.journal"), 300);
    durable
        .set_channel("stable", VersionId::new("1.5.0"))
        .unwrap();
    let before = durable.frames();
    assert!(matches!(
        durable.check_in(report("host-a", "edge", "1.5.0"), 10),
        Err(DurableFleetError::Refused(FleetError::UnknownChannel(_)))
    ));
    assert_eq!(durable.frames(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_installation_keeps_its_registered_channel() {
    // [VLD-ANB-MIDGARD-0042] [AC-45.2]
    let mut fleet = Fleet::new();
    fleet.set_channel("stable", VersionId::new("1.5.0"));
    fleet.set_channel("canary", VersionId::new("1.6.0"));
    // First sight registers the installation, pinned to the channel it reported.
    assert_eq!(
        fleet.check_in(report("host-a", "stable", "1.5.0"), 10),
        Ok(true)
    );

    assert_eq!(
        fleet.check_in(report("host-a", "edge", "1.5.0"), 11),
        Err(FleetError::UnknownChannel("edge".into()))
    );
    assert_eq!(
        fleet.check_in(report("host-a", "canary", "1.6.0"), 12),
        Err(FleetError::ChannelMismatch {
            registered: "stable".into(),
            reported: "canary".into(),
        })
    );
    let install = fleet.installations().next().unwrap();
    assert_eq!(install.channel, "stable", "the installation is unchanged");
    assert_eq!(install.version, VersionId::new("1.5.0"));
    assert_eq!(fleet.checkin("host-a").unwrap().last_seen, 10);

    // Only a configured channel and a known installation can be assigned.
    assert_eq!(
        fleet.assign_channel("host-a", "edge"),
        Err(FleetError::UnknownChannel("edge".into()))
    );
    assert_eq!(
        fleet.assign_channel("host-z", "canary"),
        Err(FleetError::UnknownInstallation("host-z".into()))
    );
    // The operator moves it; then the report on canary succeeds.
    fleet.assign_channel("host-a", "canary").unwrap();
    assert_eq!(
        fleet.check_in(report("host-a", "canary", "1.6.0"), 13),
        Ok(true)
    );
    assert!(fleet.is_converged());

    // An assignment is durable.
    let dir = scratch("assign");
    let path = dir.join("fleet.journal");
    {
        let mut durable = open(&path, 300);
        durable
            .set_channel("stable", VersionId::new("1.5.0"))
            .unwrap();
        durable
            .set_channel("canary", VersionId::new("1.6.0"))
            .unwrap();
        durable
            .check_in(report("host-a", "stable", "1.5.0"), 10)
            .unwrap();
        durable.assign_channel("host-a", "canary").unwrap();
    }
    let durable = open(&path, 300);
    assert_eq!(
        durable.fleet().installations().next().unwrap().channel,
        "canary"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unchanged_check_ins_do_not_grow_the_journal() {
    // [VLD-ANB-MIDGARD-0042] [AC-45.3] [RC-17]
    let dir = scratch("heartbeat");
    let path = dir.join("fleet.journal");
    const T0: u64 = 1_000_000;
    {
        let mut fleet = open(&path, 300);
        fleet
            .set_channel("stable", VersionId::new("1.5.0"))
            .unwrap();
        let start = fleet.frames();
        for i in 0..1_000 {
            fleet
                .check_in(report("host-a", "stable", "1.4.0"), T0 + i)
                .unwrap();
        }
        // One check-in frame, then a heartbeat at most every 300 s.
        let grown = fleet.frames() - start;
        assert!(grown <= 1 + 1_000_u64.div_ceil(300), "grew by {grown}");
        assert!(grown <= 5, "AC-45.3: at most five frames, grew by {grown}");
        assert_eq!(fleet.fleet().checkin("host-a").unwrap().last_seen, T0 + 999);
    }

    // After a restart last-seen is within one interval of the last check-in.
    let mut fleet = open(&path, 300);
    let seen = fleet.fleet().checkin("host-a").unwrap();
    assert!(
        T0 + 999 - seen.last_seen <= 300,
        "last seen {}",
        seen.last_seen
    );
    assert_eq!(seen.version, VersionId::new("1.4.0"));

    // A new version is a change: one frame, recovered exactly.
    let before = fleet.frames();
    fleet
        .check_in(report("host-a", "stable", "1.5.0"), T0 + 1_000)
        .unwrap();
    assert_eq!(fleet.frames(), before + 1);
    drop(fleet);
    let fleet = open(&path, 300);
    let seen = fleet.fleet().checkin("host-a").unwrap();
    assert_eq!(seen.version, VersionId::new("1.5.0"));
    assert_eq!(seen.last_seen, T0 + 1_000);
    assert!(fleet.fleet().is_converged());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compaction_bounds_the_journal_and_state_survives() {
    // [VLD-ANB-MIDGARD-0042] [AC-45.4] [RC-12]
    let dir = scratch("compact");
    let path = dir.join("fleet.journal");
    let mut fleet = open(&path, 300);
    fleet
        .set_channel("stable", VersionId::new("1.5.0"))
        .unwrap();
    fleet
        .set_channel("canary", VersionId::new("1.6.0"))
        .unwrap();
    fleet
        .check_in(report("host-b", "canary", "1.6.0"), 1)
        .unwrap();

    // Every check-in flips host-a's health, so every one is a change and is journalled.
    // Two installations: the threshold is 4 x 2 + 1,000 frames.
    let threshold = 4 * 2 + 1_000;
    let mut now = 10;
    while fleet.frames() < threshold {
        let healthy = now % 2 == 0;
        fleet
            .check_in(
                AgentReport::new("host-a", "stable", VersionId::new("1.4.0"), healthy),
                now,
            )
            .unwrap();
        now += 1;
    }
    assert_eq!(fleet.frames(), threshold);
    let healthy = now % 2 == 0;
    fleet
        .check_in(
            AgentReport::new("host-a", "stable", VersionId::new("1.4.0"), healthy),
            now,
        )
        .unwrap();
    assert_eq!(fleet.frames(), 1, "the journal is one snapshot frame");
    assert!(!dir.join("fleet.journal.tmp").exists());
    let before: Fleet = fleet.fleet().clone();
    drop(fleet);

    let mut recovered = open(&path, 300);
    assert_eq!(recovered.frames(), 1);
    assert_eq!(
        recovered.fleet(),
        &before,
        "the recovered fleet is the same fleet"
    );

    // The compacted journal takes appends and replays them after the snapshot.
    recovered
        .check_in(report("host-a", "stable", "1.5.0"), now + 1)
        .unwrap();
    assert_eq!(recovered.frames(), 2);
    drop(recovered);
    let reopened = open(&path, 300);
    assert_eq!(reopened.frames(), 2);
    assert!(reopened.fleet().is_converged());
    let _ = std::fs::remove_dir_all(&dir);
}
