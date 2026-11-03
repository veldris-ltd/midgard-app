//! The durable, integrity-protected [`DurableFleet`].
//!
//! Fleet state (channel desires, installation actuals and agent check-ins) is persisted
//! as a KMS-sealed, append-only journal of mutations (`SetChannel` / `Register` /
//! `MarkUpdated` / `CheckIn`), each framed, encrypted-and-authenticated at rest via a
//! [`Sealer`] (`kms::KmsSealer` — AES-256-GCM under ML-KEM-1024), `fsync`ed, with
//! `0600` perms. On [`DurableFleet::open`] the journal is decrypted and **replayed**
//! into an in-memory [`Fleet`]; a tampered record fails AEAD authentication and the
//! open fails closed — so on restart channels and check-ins are recovered and
//! re-verified. [FR-B-3] [RC-12]
//!
//! The journal records changes, not calls. A check-in is journalled as `CheckIn` only
//! when it changes the installation's version or health (or is its first); otherwise
//! last-seen is refreshed in memory and a compact `Heartbeat` is journalled at most
//! once per heartbeat interval per installation (default [`DEFAULT_HEARTBEAT`]), so
//! after a restart liveness is known to within one interval. An operator moving an
//! installation is `AssignChannel`. When the journal holds more than
//! `4 x installations + 1,000` frames, on open or after a write, it is compacted: the
//! whole state is written as one `Snapshot` frame into `fleet.journal.tmp`, fsynced
//! and renamed over the journal. Replay treats a `Snapshot` as a full reset, and
//! replays a `CheckIn` from a journal written before the channel rules existed exactly
//! as it was recorded. [VLD-ANB-MIDGARD-0042]

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use audit_writer::Sealer;
use bifrost_rollout::VersionId;

use crate::{AgentReport, CheckIn, Fleet, FleetError, Installation};

const JOURNAL_DS: &[u8] = b"midgard-fleet-state-v1";
const OP_SET_CHANNEL: u8 = 0;
const OP_REGISTER: u8 = 1;
const OP_MARK_UPDATED: u8 = 2;
const OP_CHECK_IN: u8 = 3;
/// An operator moved an installation to another channel.
const OP_ASSIGN_CHANNEL: u8 = 4;
/// An unchanged check-in: only last-seen moved.
const OP_HEARTBEAT: u8 = 5;
/// The whole fleet state; replay resets to it.
const OP_SNAPSHOT: u8 = 6;

/// The default interval, in seconds, between journalled heartbeats of an installation
/// whose check-ins change nothing.
pub const DEFAULT_HEARTBEAT: u64 = 300;

/// An error persisting or recovering the durable fleet.
#[derive(Debug)]
pub struct FleetIoError(pub String);

impl std::fmt::Display for FleetIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "durable fleet i/o: {}", self.0)
    }
}
impl std::error::Error for FleetIoError {}

/// Why a durable check-in or channel assignment did not happen: the fleet's rules
/// refused it (nothing was journalled), or it could not be journalled.
#[derive(Debug)]
pub enum DurableFleetError {
    /// The fleet's rules refused it; nothing was written.
    Refused(FleetError),
    /// The journal could not be written.
    Io(FleetIoError),
}

impl std::fmt::Display for DurableFleetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(e) => write!(f, "refused: {e}"),
            Self::Io(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for DurableFleetError {}

impl From<FleetError> for DurableFleetError {
    fn from(e: FleetError) -> Self {
        Self::Refused(e)
    }
}
impl From<FleetIoError> for DurableFleetError {
    fn from(e: FleetIoError) -> Self {
        Self::Io(e)
    }
}

/// A [`Fleet`] whose channel desires and installation records survive a restart.
pub struct DurableFleet {
    inner: Fleet,
    file: File,
    path: PathBuf,
    sealer: Box<dyn Sealer>,
    fleet_id: String,
    op_seq: u64,
    heartbeat_interval: u64,
    /// When each installation's last-seen time was last journalled.
    journalled_seen: BTreeMap<String, u64>,
}

impl DurableFleet {
    /// Open (creating if absent) the fleet journal at `path`, decrypting and
    /// replaying every mutation into memory, and compacting it if it has grown past
    /// the threshold. Fails closed on a tampered/corrupt record. Heartbeats are
    /// journalled every [`DEFAULT_HEARTBEAT`] seconds unless
    /// [`DurableFleet::with_heartbeat_interval`] says otherwise.
    pub fn open(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        fleet_id: impl Into<String>,
    ) -> Result<Self, FleetIoError> {
        let path = path.as_ref().to_path_buf();
        let fleet_id = fleet_id.into();
        let mut inner = Fleet::new();
        let mut op_seq = 0u64;

        if path.exists() {
            let mut buf = Vec::new();
            File::open(&path)
                .map_err(io)?
                .read_to_end(&mut buf)
                .map_err(io)?;
            let mut pos = 0usize;
            while pos < buf.len() {
                if pos + 4 > buf.len() {
                    return Err(FleetIoError("truncated frame header".into()));
                }
                let len = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
                    as usize;
                pos += 4;
                let end = pos
                    .checked_add(len)
                    .filter(|&e| e <= buf.len())
                    .ok_or_else(|| FleetIoError("truncated frame body".into()))?;
                let plaintext = sealer
                    .unseal(&aad(&fleet_id, op_seq), &buf[pos..end])
                    .map_err(|e| FleetIoError(e.to_string()))?;
                pos = end;
                apply(&mut inner, decode_op(&plaintext)?);
                op_seq += 1;
            }
        }

        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&path).map_err(io)?;

        let journalled_seen = inner
            .checkins()
            .map(|(id, c)| (id.to_string(), c.last_seen))
            .collect();
        let mut fleet = Self {
            inner,
            file,
            path,
            sealer,
            fleet_id,
            op_seq,
            heartbeat_interval: DEFAULT_HEARTBEAT,
            journalled_seen,
        };
        fleet.compact_if_due()?;
        Ok(fleet)
    }

    /// Journal an unchanged installation's last-seen time at most once every `secs`
    /// seconds (0 journals it on every check-in).
    pub fn with_heartbeat_interval(mut self, secs: u64) -> Self {
        self.heartbeat_interval = secs;
        self
    }

    /// Declare (or update) a channel's desired version, durably.
    pub fn set_channel(
        &mut self,
        name: impl Into<String>,
        desired: VersionId,
    ) -> Result<(), FleetIoError> {
        let name = name.into();
        let mut b = vec![OP_SET_CHANNEL];
        put_str(&mut b, &name);
        put_str(&mut b, &desired.0);
        self.journal(&b)?;
        self.inner.set_channel(name, desired);
        self.compact_if_due()
    }

    /// Register or replace an installation record, durably.
    pub fn register(&mut self, install: Installation) -> Result<(), FleetIoError> {
        let mut b = vec![OP_REGISTER];
        put_str(&mut b, &install.id);
        put_str(&mut b, &install.channel);
        put_str(&mut b, &install.version.0);
        self.journal(&b)?;
        self.inner.register(install);
        self.compact_if_due()
    }

    /// Record that an installation now runs `version` (a check-in), durably.
    pub fn mark_updated(
        &mut self,
        install_id: &str,
        version: VersionId,
    ) -> Result<(), FleetIoError> {
        let mut b = vec![OP_MARK_UPDATED];
        put_str(&mut b, install_id);
        put_str(&mut b, &version.0);
        self.journal(&b)?;
        self.inner.mark_updated(install_id, version);
        self.compact_if_due()
    }

    /// Record an agent's check-in, stamped with the manager's clock `now`, durably:
    /// after a restart the installation's reported version and health are recovered,
    /// and its last-seen time to within the heartbeat interval. [FR-B-3] [RC-17]
    ///
    /// The fleet's channel rules ([`Fleet::check_in`]) are applied first, so a refused
    /// report journals nothing. A check-in that changes the installation is journalled
    /// in full; one that does not journals a heartbeat only when the installation's
    /// last journalled time is a heartbeat interval old. Memory changes only after
    /// the journal write. [VLD-ANB-MIDGARD-0042]
    pub fn check_in(&mut self, report: AgentReport, now: u64) -> Result<(), DurableFleetError> {
        let changed = self.inner.vet_check_in(&report)?;
        let due = self
            .journalled_seen
            .get(&report.install_id)
            .is_none_or(|&at| now.saturating_sub(at) >= self.heartbeat_interval);
        if changed {
            let mut b = vec![OP_CHECK_IN];
            put_str(&mut b, &report.install_id);
            put_str(&mut b, &report.channel);
            put_str(&mut b, &report.version.0);
            b.push(u8::from(report.healthy));
            b.extend_from_slice(&now.to_le_bytes());
            self.journal(&b)?;
        } else if due {
            let mut b = vec![OP_HEARTBEAT];
            put_str(&mut b, &report.install_id);
            b.extend_from_slice(&now.to_le_bytes());
            self.journal(&b)?;
        }
        if changed || due {
            self.journalled_seen.insert(report.install_id.clone(), now);
        }
        self.inner.record_check_in(report, now);
        self.compact_if_due()?;
        Ok(())
    }

    /// Move an installation to another declared channel, durably: the operator's
    /// action ([`Fleet::assign_channel`]). [VLD-ANB-MIDGARD-0042]
    pub fn assign_channel(
        &mut self,
        install_id: &str,
        channel: &str,
    ) -> Result<(), DurableFleetError> {
        self.inner.vet_assign_channel(install_id, channel)?;
        let mut b = vec![OP_ASSIGN_CHANNEL];
        put_str(&mut b, install_id);
        put_str(&mut b, channel);
        self.journal(&b)?;
        self.inner.record_assign_channel(install_id, channel);
        self.compact_if_due()?;
        Ok(())
    }

    /// Rewrite the journal as one snapshot of the whole state: written to
    /// `fleet.journal.tmp` (`0600`), fsynced and renamed over the journal, which then
    /// holds one frame. A failure before the rename leaves the journal as it was.
    /// [VLD-ANB-MIDGARD-0042]
    pub fn compact(&mut self) -> Result<(), FleetIoError> {
        let sealed = self
            .sealer
            .seal(&aad(&self.fleet_id, 0), &encode_snapshot(&self.inner))
            .map_err(|e| FleetIoError(e.to_string()))?;
        let tmp = self.path.with_extension("journal.tmp");
        let mut opts = OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp).map_err(io)?;
        file.write_all(&frame(&sealed)).map_err(io)?;
        file.sync_all().map_err(io)?;
        std::fs::rename(&tmp, &self.path).map_err(io)?;
        #[cfg(unix)]
        if let Some(dir) = self.path.parent() {
            File::open(dir).and_then(|d| d.sync_all()).map_err(io)?;
        }
        // The handle the snapshot went through is now the journal; appends continue
        // on it (it is positioned at its end).
        self.file = file;
        self.op_seq = 1;
        Ok(())
    }

    /// The number of frames in the journal (one after a compaction).
    pub fn frames(&self) -> u64 {
        self.op_seq
    }

    /// The recovered in-memory fleet (drift/reconcile/read surface).
    pub fn fleet(&self) -> &Fleet {
        &self.inner
    }
    /// The journal path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn compact_if_due(&mut self) -> Result<(), FleetIoError> {
        let installs = self.inner.installations().count() as u64;
        if self.op_seq > installs.saturating_mul(4).saturating_add(1_000) {
            self.compact()?;
        }
        Ok(())
    }

    fn journal(&mut self, plaintext: &[u8]) -> Result<(), FleetIoError> {
        let sealed = self
            .sealer
            .seal(&aad(&self.fleet_id, self.op_seq), plaintext)
            .map_err(|e| FleetIoError(e.to_string()))?;
        self.file.write_all(&frame(&sealed)).map_err(io)?;
        self.file.sync_all().map_err(io)?;
        self.op_seq += 1;
        Ok(())
    }
}

fn frame(sealed: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(4 + sealed.len());
    frame.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
    frame.extend_from_slice(sealed);
    frame
}

fn io(e: std::io::Error) -> FleetIoError {
    FleetIoError(e.to_string())
}

fn aad(fleet_id: &str, seq: u64) -> Vec<u8> {
    let mut a = Vec::with_capacity(JOURNAL_DS.len() + 4 + fleet_id.len() + 8);
    a.extend_from_slice(JOURNAL_DS);
    a.extend_from_slice(&(fleet_id.len() as u32).to_le_bytes());
    a.extend_from_slice(fleet_id.as_bytes());
    a.extend_from_slice(&seq.to_le_bytes());
    a
}

// --- codec ----------------------------------------------------------------

enum Op {
    SetChannel(String, VersionId),
    Register(Installation),
    MarkUpdated(String, VersionId),
    CheckIn(AgentReport, u64),
    AssignChannel(String, String),
    Heartbeat(String, u64),
    Snapshot(Fleet),
}

fn apply(fleet: &mut Fleet, op: Op) {
    match op {
        Op::SetChannel(name, v) => fleet.set_channel(name, v),
        Op::Register(i) => fleet.register(i),
        Op::MarkUpdated(id, v) => fleet.mark_updated(&id, v),
        // As recorded: a journal from before the channel rules may hold check-ins they
        // would now refuse, and what it recorded is still the state.
        Op::CheckIn(report, now) => fleet.record_check_in(report, now),
        Op::AssignChannel(id, channel) => fleet.record_assign_channel(&id, &channel),
        Op::Heartbeat(id, now) => fleet.touch(&id, now),
        Op::Snapshot(state) => *fleet = state,
    }
}

/// The whole state: counted channels, installations and check-ins.
fn encode_snapshot(fleet: &Fleet) -> Vec<u8> {
    let mut b = vec![OP_SNAPSHOT];
    let channels: Vec<_> = fleet.channels().collect();
    put_count(&mut b, channels.len());
    for (name, desired) in channels {
        put_str(&mut b, name);
        put_str(&mut b, &desired.0);
    }
    let installs: Vec<_> = fleet.installations().collect();
    put_count(&mut b, installs.len());
    for i in installs {
        put_str(&mut b, &i.id);
        put_str(&mut b, &i.channel);
        put_str(&mut b, &i.version.0);
    }
    let checkins: Vec<_> = fleet.checkins().collect();
    put_count(&mut b, checkins.len());
    for (id, c) in checkins {
        put_str(&mut b, id);
        put_str(&mut b, &c.version.0);
        b.push(u8::from(c.healthy));
        b.extend_from_slice(&c.last_seen.to_le_bytes());
    }
    b
}

fn decode_snapshot(r: &mut Reader<'_>) -> Result<Fleet, FleetIoError> {
    let mut channels = BTreeMap::new();
    for _ in 0..r.u32()? {
        channels.insert(r.string()?, VersionId(r.string()?));
    }
    let mut installs = BTreeMap::new();
    for _ in 0..r.u32()? {
        let i = Installation {
            id: r.string()?,
            channel: r.string()?,
            version: VersionId(r.string()?),
        };
        installs.insert(i.id.clone(), i);
    }
    let mut checkins = BTreeMap::new();
    for _ in 0..r.u32()? {
        let id = r.string()?;
        let version = VersionId(r.string()?);
        let healthy = health(r.u8()?)?;
        let last_seen = r.u64()?;
        checkins.insert(
            id,
            CheckIn {
                version,
                healthy,
                last_seen,
            },
        );
    }
    Ok(Fleet::from_parts(channels, installs, checkins))
}

fn health(flag: u8) -> Result<bool, FleetIoError> {
    match flag {
        0 => Ok(false),
        1 => Ok(true),
        h => Err(FleetIoError(format!("invalid health flag {h}"))),
    }
}

fn put_count(o: &mut Vec<u8>, n: usize) {
    o.extend_from_slice(&(n as u32).to_le_bytes());
}

fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8, FleetIoError> {
        let b = self.take(1)?;
        Ok(b[0])
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], FleetIoError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| FleetIoError("codec truncated".into()))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn string(&mut self) -> Result<String, FleetIoError> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| FleetIoError("codec invalid utf8".into()))
    }
    fn u32(&mut self) -> Result<u32, FleetIoError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, FleetIoError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }
}

fn decode_op(b: &[u8]) -> Result<Op, FleetIoError> {
    let mut r = Reader { b, pos: 0 };
    let op = match r.u8()? {
        OP_SET_CHANNEL => Op::SetChannel(r.string()?, VersionId(r.string()?)),
        OP_REGISTER => Op::Register(Installation {
            id: r.string()?,
            channel: r.string()?,
            version: VersionId(r.string()?),
        }),
        OP_MARK_UPDATED => Op::MarkUpdated(r.string()?, VersionId(r.string()?)),
        OP_CHECK_IN => {
            let install_id = r.string()?;
            let channel = r.string()?;
            let version = VersionId(r.string()?);
            let healthy = health(r.u8()?)?;
            let now = r.u64()?;
            Op::CheckIn(
                AgentReport {
                    install_id,
                    channel,
                    version,
                    healthy,
                },
                now,
            )
        }
        OP_ASSIGN_CHANNEL => Op::AssignChannel(r.string()?, r.string()?),
        OP_HEARTBEAT => Op::Heartbeat(r.string()?, r.u64()?),
        OP_SNAPSHOT => Op::Snapshot(decode_snapshot(&mut r)?),
        t => return Err(FleetIoError(format!("unknown op tag {t}"))),
    };
    if r.pos != b.len() {
        return Err(FleetIoError("codec trailing bytes".into()));
    }
    Ok(op)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kms::{Kms, KmsSealer, SoftwareTrustRoot};
    use pqc_agility::backend::fips::provider as fips_provider;
    use pqc_agility::OsRng;

    fn scratch(tag: &str) -> PathBuf {
        let uniq = Box::into_raw(Box::new(0u8)) as usize;
        let dir = std::env::temp_dir().join(format!(
            "bifrost-fleet-unit-{tag}-{}-{uniq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Frames exactly as a build before the channel rules wrote them: a check-in per
    /// call, one on a channel that is not declared, and one that changes channel.
    fn legacy_check_in(id: &str, channel: &str, version: &str, now: u64) -> Vec<u8> {
        let mut b = vec![OP_CHECK_IN];
        put_str(&mut b, id);
        put_str(&mut b, channel);
        put_str(&mut b, version);
        b.push(1);
        b.extend_from_slice(&now.to_le_bytes());
        b
    }

    #[test]
    fn a_journal_from_an_earlier_build_loads_and_is_compacted() {
        // [VLD-ANB-MIDGARD-0042] [AC-45.4] [RC-12]
        let provider = fips_provider();
        let root = SoftwareTrustRoot::generate(&provider, "fleet-root", &mut OsRng).unwrap();
        let sealer = KmsSealer::new(Kms::new(provider, root));
        let sealer: std::sync::Arc<dyn Sealer> = std::sync::Arc::new(sealer);
        struct Shared(std::sync::Arc<dyn Sealer>);
        impl Sealer for Shared {
            fn seal(&self, aad: &[u8], p: &[u8]) -> Result<Vec<u8>, audit_writer::AuditError> {
                self.0.seal(aad, p)
            }
            fn unseal(&self, aad: &[u8], s: &[u8]) -> Result<Vec<u8>, audit_writer::AuditError> {
                self.0.unseal(aad, s)
            }
        }
        let dir = scratch("legacy");
        let path = dir.join("fleet.journal");

        let expected = {
            let mut old = DurableFleet::open(&path, Box::new(Shared(sealer.clone())), "f").unwrap();
            let mut b = vec![OP_SET_CHANNEL];
            put_str(&mut b, "stable");
            put_str(&mut b, "1.5.0");
            old.journal(&b).unwrap();
            old.journal(&legacy_check_in("host-a", "stable", "1.4.0", 10))
                .unwrap();
            old.journal(&legacy_check_in("host-b", "beta", "2.0.0", 11))
                .unwrap();
            old.journal(&legacy_check_in("host-a", "edge", "1.4.0", 12))
                .unwrap();
            for now in 0..1_010 {
                old.journal(&legacy_check_in("host-a", "edge", "1.4.0", 100 + now))
                    .unwrap();
            }
            assert_eq!(old.frames(), 1_014);
            let mut fleet = Fleet::new();
            fleet.set_channel("stable", VersionId::new("1.5.0"));
            fleet.record_check_in(
                AgentReport::new("host-b", "beta", VersionId::new("2.0.0"), true),
                11,
            );
            fleet.record_check_in(
                AgentReport::new("host-a", "edge", VersionId::new("1.4.0"), true),
                1_109,
            );
            fleet
        };

        // 1,014 frames is past 4 x 2 + 1,000: the open replays them as recorded and
        // compacts the journal to one snapshot frame.
        let reopened = DurableFleet::open(&path, Box::new(Shared(sealer.clone())), "f").unwrap();
        assert_eq!(reopened.fleet(), &expected);
        assert_eq!(reopened.frames(), 1);
        drop(reopened);
        let again = DurableFleet::open(&path, Box::new(Shared(sealer)), "f").unwrap();
        assert_eq!(again.fleet(), &expected);
        assert_eq!(again.frames(), 1);
        assert!(!dir.join("fleet.journal.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
